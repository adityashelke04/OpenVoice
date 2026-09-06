//! The composition root.
//!
//! Owns every adapter and drives the pure state machine, exactly as `ov-cli` does.
//! The only difference is where effects go: the CLI prints them, this emits them to
//! the webview. `ov-core` does not know either exists, which is the entire point of
//! the port boundary — and it is why the CLI still works as a headless test harness
//! for the same pipeline.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ov_core::event::Event;
use ov_core::ports::{
    AppContext, AudioSource, DecodeHint, HistoryStore, HotkeyEvent, HotkeyListener, LevelFrame,
    Pcm16k, TextSink, Transcriber, Utterance,
};
use ov_core::session::{Effect, Input, SessionMachine};
use ov_core::types::{Millis, Outcome, SessionId};
use ov_format::profile::{self, Profile};
use ov_format::Formatter;

/// What the UI needs to know that is not carried by an event.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Ready {
    pub model: String,
    pub device: String,
    pub shortcut: String,
    pub mic: String,
}

/// How far a model download has got.
///
/// `total` is the catalogue's figure when the server sends no `Content-Length`,
/// so the bar shows a real proportion rather than dropping to indeterminate
/// halfway through a 465 MB transfer.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub model: String,
    pub done: u64,
    pub total: u64,
}

/// Anything the shell must do in response to the engine.
pub trait Shell: Send + Sync + 'static {
    /// Publish a domain event to the UI.
    fn emit(&self, event: &Event);
    /// Show or hide the floating overlay.
    fn set_overlay_visible(&self, visible: bool);
    /// Tell the Flow Bar whether the microphone is being held open without a key.
    ///
    /// A latched session and a held one are otherwise identical on screen, and
    /// the difference between them is the only one that matters when you take
    /// your hand off the keyboard. A bar that cannot say which it is showing is
    /// a bar that lets people walk away from an open microphone.
    fn set_latched(&self, latched: bool);
    /// The user pressed the cancel key.
    ///
    /// Distinct from a session being cancelled, and that is the whole point: the
    /// key is pressed just as often when nothing is running, and the shell has
    /// something to do about it that the session machine does not know or care
    /// about. Today that is closing the Flow Menu, which cannot hear the
    /// keystroke itself — the bar has no focus, so no key event ever reaches its
    /// webview.
    ///
    /// Defaulted to nothing so the test shells in this crate do not each have to
    /// grow an empty body for a concern they have no opinion on.
    fn on_cancel_key(&self) {}
}

/// Profiles and their compiled formatters, replaced as a unit.
///
/// Kept together because they must never disagree: a formatter compiled from a
/// profile that has since been edited would format with stale rules while the app
/// reports the new ones.
struct Rules {
    profiles: Vec<Profile>,
    formatters: Vec<(String, Formatter)>,
    /// Proper nouns offered to the decoder. Rebuilt with the rest, so a term the
    /// user adds starts helping the *model* on the next utterance, not just the
    /// post-processing.
    hints: Vec<String>,
}

impl Rules {
    fn build(settings: &crate::settings::Settings) -> Self {
        // User terms first so they win over the builtins — `Dictionary::compile`
        // keeps the first writer for a spoken phrase.
        let mut entries = settings.dictionary.clone();
        entries.extend(ov_format::dictionary::builtin_entries());

        let profiles = settings.profiles.clone();
        let formatters = profiles
            .iter()
            .map(|p| (p.name.clone(), Formatter::new(p.clone(), &entries)))
            .collect();
        let hints = ov_format::dictionary::hint_terms(&entries);
        Self {
            profiles,
            formatters,
            hints,
        }
    }
}

/// Compile the configured redaction patterns, reporting any that were rejected.
///
/// A bad pattern is skipped rather than fatal — these are hand-edited in a TOML
/// file, and refusing to start over one would trade a slightly less redacted
/// history for no dictation at all.
fn build_redactor(
    privacy: &ov_core::config::PrivacyConfig,
) -> (ov_core::redact::Redactor, Vec<String>) {
    ov_core::redact::Redactor::compile(&privacy.redact_patterns)
}

pub struct Engine {
    audio: ov_audio::CpalAudioSource,
    /// The speech backend.
    ///
    /// Named concretely rather than held behind `Arc<dyn Transcriber>`: there is
    /// one backend now, and a trait object that is only ever one type is
    /// indirection pretending to be flexibility. The port still exists and
    /// `ov-core` still depends on it -- that is what made this swap cheap -- but
    /// the composition root can say what it actually built.
    transcriber: ov_asr::sherpa::SherpaTranscriber,
    sink: ov_input::WinTextSink,
    apps: ov_input::WinForeground,
    /// Profiles and formatters live behind one lock and are replaced together.
    /// Rebuilding them live is what makes "changes apply to the next thing you
    /// dictate" true rather than a claim that quietly requires a restart.
    rules: Mutex<Rules>,
    /// Captured audio waiting to be transcribed, by session.
    ///
    /// A single slot, which is what this was, holds exactly one utterance — but
    /// the session machine keeps an ordered queue of post-capture sessions and
    /// only issues `Transcribe` for the front of it. Dictate three times in
    /// quick succession and the third capture overwrote the second's buffer:
    /// session 2 was then transcribed from session 3's audio, injecting the
    /// wrong words, and session 3 found the slot empty and failed. Keyed by
    /// session, each utterance keeps its own audio until its turn comes.
    captured: Mutex<HashMap<SessionId, Pcm16k>>,
    /// Durable history. Injected as a port, so the engine neither knows nor cares
    /// that it is SQLite.
    history: Arc<dyn HistoryStore>,
    /// Last delivered text, for the tray's "paste last transcript".
    ///
    /// Deliberately the *unredacted* text: this is what gets pasted when the user
    /// asks for it again, and handing them `[redacted]` instead of their own words
    /// would be a bug. It is in memory only and dies with the process.
    last_text: Mutex<String>,
    /// Patterns stripped from transcripts before they are written to history.
    ///
    /// Behind a lock and rebuilt on save, like the formatter rules, so editing the
    /// list takes effect on the next utterance rather than at the next restart.
    redactor: Mutex<ov_core::redact::Redactor>,
    paste_threshold: usize,
    /// Forced transcription language, or `None` to auto-detect.
    ///
    /// Retained but unused: Parakeet v2 is English-only, so there is no language
    /// to force and none to detect. Kept because `Config` is versioned and
    /// persisted, and Parakeet v3 is multilingual on the same runtime.
    language: Mutex<Option<String>>,
    start: Instant,
    shell: Arc<dyn Shell>,
    /// Sender into the session machine, for inputs that do not come from the
    /// keyboard hook.
    ///
    /// A `OnceLock` because the channel is created after the engine it feeds —
    /// `start` builds the engine, then the channel, then the threads. Set once,
    /// immediately, and never replaced.
    inputs: std::sync::OnceLock<std::sync::mpsc::Sender<Input>>,
    /// The live keyboard hook, retained so the bound key can be changed while the
    /// app is running.
    ///
    /// It used to be dropped at the end of `start`. Nothing broke -- the listener
    /// is a handle onto process-wide state, so the hook survived -- but with no
    /// owner there was nothing to call `rebind` on, and that is exactly why
    /// changing the shortcut did nothing until the next launch.
    hotkey: ov_input::WinHotkeyListener,
    /// How the user's chord is configured, so `toggle` can reproduce it.
    ///
    /// Behind a lock rather than a plain field because the Settings screen can
    /// change it at any moment, and `toggle` must synthesise the press pattern the
    /// *current* mode expects, not the one that was set at launch.
    activation: Mutex<ov_core::config::ActivationMode>,
    /// Whether `toggle` currently believes it is holding the key down.
    ///
    /// The session machine is driven entirely by key transitions, and it
    /// deliberately does not care where they come from. Clicking the bar
    /// therefore has to produce the same press/release pattern a real key would,
    /// which means remembering which half of it we are in.
    synthetic_down: std::sync::atomic::AtomicBool,
    /// Turns tap patterns on the shortcut into hands-free dictation.
    ///
    /// Lives here rather than in the hook closure because two things need it:
    /// the hook, which feeds it key transitions, and `toggle`, which has to be
    /// able to close a latched session when the user clicks the Flow Bar. See
    /// [`crate::taplatch`].
    latch: Mutex<crate::taplatch::TapLatch>,
}

impl Engine {
    fn now(&self) -> Millis {
        Millis(self.start.elapsed().as_millis() as u64)
    }

    /// Format a transcript with the named profile, falling back to the first.
    ///
    /// Formatting happens inside the lock rather than handing out a reference, so
    /// the rules can be swapped underneath without borrow gymnastics. The work is
    /// pure string manipulation measured in microseconds.
    fn format(&self, profile: &str, raw: &str) -> String {
        let r = self.rules.lock().expect("rules");
        let f = r
            .formatters
            .iter()
            .find(|(n, _)| n == profile)
            .map(|(_, f)| f)
            .or_else(|| r.formatters.first().map(|(_, f)| f));
        match f {
            Some(f) => f.format(raw),
            None => raw.to_string(),
        }
    }

    /// Which profile applies to an executable.
    fn profile_for(&self, exe: &str) -> String {
        let r = self.rules.lock().expect("rules");
        profile::select(&r.profiles, exe)
            .map(|p| p.name.clone())
            .or_else(|| r.profiles.last().map(|p| p.name.clone()))
            .unwrap_or_else(|| "prose".into())
    }

    /// Rebuild profiles and formatters from edited settings.
    pub fn reload_rules(&self, settings: &crate::settings::Settings) {
        *self.rules.lock().expect("rules") = Rules::build(settings);
        tracing::info!(
            terms = settings.dictionary.len(),
            profiles = settings.profiles.len(),
            "writing rules reloaded"
        );
        self.reload_redactor(settings);
    }

    /// Recompile the redaction patterns from edited settings.
    pub fn reload_redactor(&self, settings: &crate::settings::Settings) {
        let (redactor, errors) = build_redactor(&settings.config.privacy);
        *self.redactor.lock().expect("redactor mutex") = redactor;
        for e in errors {
            tracing::warn!("{e}");
        }
    }

    /// Apply an edited language preference to the next dictation.
    ///
    /// No restart needed, unlike the model: this only changes what gets passed
    /// to the already-running sidecar on the next decode.
    pub fn reload_language(&self, settings: &crate::settings::Settings) {
        *self.language.lock().expect("language mutex") = settings.config.language.clone();
        tracing::info!(language = ?settings.config.language, "transcription language changed");
    }

    /// Apply an edited shortcut and activation style to the running app.
    ///
    /// This is the counterpart to [`Engine::reload_rules`] and
    /// [`Engine::reload_language`], and it was the one that was missing. Saving a
    /// new shortcut wrote it to `settings.toml` and stopped there: the keyboard
    /// hook went on matching whichever key it had been given at launch, so the new
    /// key did nothing and the old one still worked -- with the Settings screen
    /// showing the new one as if it were in force.
    ///
    /// Neither half needs a restart. The hook compares one atomic on every
    /// keystroke, so rebinding is a store; the session machine owns its activation
    /// mode, so it is told down the same channel its other inputs arrive on.
    pub fn reload_hotkey(&self, settings: &crate::settings::Settings) -> Result<(), String> {
        use ov_core::ports::HotkeyListener;

        let chord = settings.config.chord;
        self.hotkey.rebind(&chord).map_err(|e| e.to_string())?;

        let activation = settings.config.activation;
        *self.activation.lock().expect("activation mutex") = activation;
        // Forget any half-finished click-to-talk. In push-to-talk the Flow Bar's
        // hold spans two clicks, so a mode change landing between them would leave
        // this set with no second click coming -- and the next click after that
        // would be read as the release of a press that never happened.
        self.synthetic_down
            .store(false, std::sync::atomic::Ordering::SeqCst);
        if let Some(tx) = self.inputs.get() {
            let _ = tx.send(Input::ActivationChanged {
                mode: activation,
                at: self.now(),
            });
        }

        tracing::info!(
            key = %chord.key.label(),
            exclusive = chord.exclusive,
            ?activation,
            "shortcut applied without restart"
        );
        Ok(())
    }

    /// Discard whatever session is in flight.
    ///
    /// The same input the Escape key produces, so there is exactly one cancel
    /// path through the state machine rather than a second one that has to be
    /// kept in step. Harmless when nothing is running: `Input::Cancelled` from
    /// idle is a no-op in `session.rs`.
    pub fn cancel(&self) {
        // Escape ends a latched session as surely as a tap does, and the latch
        // has to be told or it would keep believing the microphone was open --
        // making the next tap "stop" a session that had already gone.
        self.latch.lock().expect("latch").forget();
        self.shell.set_latched(false);
        if let Some(tx) = self.inputs.get() {
            let _ = tx.send(Input::Cancelled { at: self.now() });
        }
    }

    /// Start or stop a dictation from a click rather than the chord.
    ///
    /// Wispr Flow's bar starts a dictation when you click it, and it is the right
    /// affordance for the same reason their bar has it: a floating control that
    /// only reports is a status light, and one you can press is an instrument.
    /// The shortcut stays the fast path; this is the discoverable one.
    ///
    /// Synthesises the key transitions rather than reaching into the machine, so
    /// there is one activation path and no second set of rules to keep in step —
    /// the same argument `cancel` makes. That means honouring the configured
    /// style, because the two modes read a press differently: in toggle mode a
    /// complete tap starts and a second complete tap stops, while push-to-talk
    /// needs the press and the release separated by however long the user speaks.
    ///
    /// Sampling the foreground application here is correct and not a
    /// coincidence: the overlay is `WS_EX_NOACTIVATE`, so clicking it never takes
    /// focus, and the window the user was typing into is still the foreground
    /// one. The property that makes this window safe is the property that makes
    /// this method possible.
    pub fn toggle(&self) {
        use ov_core::config::ActivationMode;
        use std::sync::atomic::Ordering;

        let Some(tx) = self.inputs.get() else { return };
        let at = self.now();

        // A latched session has no key being held, so there is no press pattern
        // to reproduce -- only a release to deliver. Handled before the mode
        // below because `synthetic_down` describes a hold this session never had,
        // and consulting it here would leave the click doing nothing.
        {
            let mut latch = self.latch.lock().expect("latch");
            if latch.is_latched() {
                latch.forget();
                self.shell.set_latched(false);
                let _ = tx.send(Input::HotkeyReleased { at });
                return;
            }
        }

        let press = |tx: &std::sync::mpsc::Sender<Input>| {
            let app = self.apps.foreground().unwrap_or_default();
            let profile = self.profile_for(&app.exe);
            let _ = tx.send(Input::HotkeyPressed { at, app, profile });
        };

        let activation = *self.activation.lock().expect("activation mutex");
        match activation {
            // One complete tap per click. The machine's own toggle handling then
            // does the starting and stopping.
            ActivationMode::Toggle => {
                press(tx);
                let _ = tx.send(Input::HotkeyReleased { at });
            }
            // Hold-to-talk, with the hold spanning two clicks.
            ActivationMode::PushToTalk => {
                if self.synthetic_down.swap(true, Ordering::SeqCst) {
                    self.synthetic_down.store(false, Ordering::SeqCst);
                    let _ = tx.send(Input::HotkeyReleased { at });
                } else {
                    press(tx);
                }
            }
        }
    }

    /// Text of the most recent successful dictation.
    pub fn last_text(&self) -> String {
        self.last_text.lock().map(|t| t.clone()).unwrap_or_default()
    }

    /// Re-deliver the last transcript to whatever now has focus.
    ///
    /// Silence here is the wrong answer: a menu item that does nothing when there
    /// is nothing to paste is indistinguishable from one that is broken.
    pub fn paste_last(&self) {
        let text = self.last_text();
        if text.is_empty() {
            self.shell.emit(&Event::Notice {
                level: ov_core::event::NoticeLevel::Info,
                message: "Nothing to paste yet — dictate something first.".into(),
            });
            return;
        }
        let mode = ov_input::mode_for(&text, self.paste_threshold);
        match self.sink.inject(&text, mode) {
            Ok(r) => tracing::info!(chars = r.chars, "pasted last transcript"),
            Err(e) => {
                tracing::warn!(error = %e, "paste last failed");
                self.shell.emit(&Event::Notice {
                    level: ov_core::event::NoticeLevel::Warn,
                    message: "Copied to clipboard — press Ctrl+V".into(),
                });
            }
        }
    }
}

/// Build every adapter, start the hotkey hook, and run the event loop on a
/// background thread. Returns once the model is warm.
///
pub fn start(
    shell: Arc<dyn Shell>,
    settings: &crate::settings::Settings,
    history: Arc<dyn HistoryStore>,
) -> Result<(Arc<Engine>, Ready), String> {
    let config = settings.config.clone();

    // Which model, with a fallback that is guaranteed to exist.
    //
    // A user can select a downloaded model and then delete its files, or a
    // transfer can be interrupted, or settings.toml can be hand-edited to a name
    // that was never valid. None of those may leave the app unable to
    // transcribe: the bundled model ships in the installer and is always on
    // disk, so there is no state in which OpenVoice legitimately cannot hear
    // you. Falling back is loud in the log and silent to the user, who finds the
    // Models screen already showing which one is actually running.
    let user_models = crate::history::data_dir().join("models");
    let bundled = ov_asr::catalog::default_spec();
    let wanted = ov_asr::catalog::find(&settings.model).unwrap_or_else(|| {
        tracing::warn!(model = %settings.model, "unknown model in settings; using the bundled one");
        bundled
    });

    let (spec, dir) = match ov_asr::locate::model_dir(wanted, &user_models) {
        Ok(d) => (wanted, d),
        Err(e) if wanted.id != bundled.id => {
            tracing::warn!(model = wanted.id, error = %e, "falling back to the bundled model");
            let d = ov_asr::locate::model_dir(bundled, &user_models).map_err(|e| e.to_string())?;
            (bundled, d)
        }
        Err(e) => return Err(e.to_string()),
    };
    tracing::info!(model = spec.id, dir = %dir.display(), "loading the speech model");

    // Retention has to be passed explicitly. With the old sidecar it fell out of
    // how audio crossed the process boundary -- a WAV was written either way, and
    // "keep recordings" merely spared it from deletion. In-process nothing is
    // written unless it is asked for, so omitting this would silently turn the
    // setting off rather than silently on.
    let retain = config
        .privacy
        .retain_audio
        .then(|| crate::history::data_dir().join("audio"));
    if let Some(d) = &retain {
        tracing::warn!(dir = %d.display(), "keeping recordings on disk; this is off by default");
    }

    let transcriber = ov_asr::sherpa::SherpaTranscriber::with_retention(spec, dir, retain)
        .map_err(|e| e.to_string())?;

    transcriber.warm().map_err(|e| e.to_string())?;

    let rules = Rules::build(settings);
    let (redactor, redact_errors) = build_redactor(&config.privacy);
    for e in &redact_errors {
        tracing::warn!("{e}");
    }

    let audio =
        ov_audio::CpalAudioSource::new(config.input_device.clone()).map_err(|e| e.to_string())?;
    let mic = audio
        .devices()
        .ok()
        .and_then(|d| d.first().cloned())
        .unwrap_or_else(|| "System default".into());

    let ready = Ready {
        // What actually loaded, not what settings.toml happens to say. Those two
        // drifted apart the moment the model stopped being selectable, and the
        // Engine card was reporting "base.en" while Parakeet was doing the work.
        model: transcriber.model_id(),
        device: transcriber.model_id(),
        // The key actually bound, not a guess. This was the literal string
        // "Right Ctrl", which was true only until someone rebound it — and the
        // Hub shows this on the home screen as the instruction for how to use
        // the app, so being wrong here is worse than being absent.
        shortcut: config.chord.key.label().into(),
        mic,
    };

    // Built before the engine so it can be moved in and kept. The hook itself is
    // not installed until `start` below.
    let hotkey = ov_input::WinHotkeyListener::new(config.chord);

    let engine = Arc::new(Engine {
        audio,
        transcriber,
        sink: ov_input::WinTextSink::new(config.paste_threshold_chars),
        apps: ov_input::WinForeground,
        rules: Mutex::new(rules),
        captured: Mutex::new(HashMap::new()),
        history,
        last_text: Mutex::new(String::new()),
        redactor: Mutex::new(redactor),
        paste_threshold: config.paste_threshold_chars,
        language: Mutex::new(config.language.clone()),
        start: Instant::now(),
        shell,
        inputs: std::sync::OnceLock::new(),
        hotkey,
        activation: Mutex::new(config.activation),
        synthetic_down: std::sync::atomic::AtomicBool::new(false),
        latch: Mutex::new(crate::taplatch::TapLatch::default()),
    });

    let (tx, rx) = channel::<Input>();
    // Hand the engine its own way in, so commands that are not keystrokes — the
    // Flow Bar's cancel button — reach the same state machine the hook does.
    let _ = engine.inputs.set(tx.clone());

    // Hotkey -> machine input. The foreground application is sampled on *press*:
    // by injection time the user may have switched windows, and the profile must
    // reflect where they were speaking.
    {
        let tx = tx.clone();
        let e = engine.clone();
        engine
            .hotkey
            .start(Arc::new(move |event| {
                use crate::taplatch::{OnPress, OnRelease};
                use ov_core::config::ActivationMode;

                let at = e.now();

                // Tap gestures only mean anything in hold-to-talk. In toggle mode
                // a tap already starts and a second one already stops, so there is
                // nothing for a latch to add and every reason not to add it.
                let push_to_talk = matches!(
                    *e.activation.lock().expect("activation mutex"),
                    ActivationMode::PushToTalk
                );

                let pressed = |e: &Engine| {
                    let app = e.apps.foreground().unwrap_or_default();
                    let profile = e.profile_for(&app.exe);
                    Input::HotkeyPressed { at, app, profile }
                };

                match event {
                    HotkeyEvent::Pressed if push_to_talk => {
                        let decision = e.latch.lock().expect("latch").press(at.0);
                        match decision {
                            OnPress::Start => {
                                let _ = tx.send(pressed(&e));
                            }
                            OnPress::LatchOpen => {
                                e.shell.set_latched(true);
                                // The first tap of the gesture started a session
                                // and its release stopped it. That stub is a few
                                // tens of milliseconds of audio nobody asked for,
                                // and transcribing it would put a spurious notice
                                // on the bar at the exact moment the user is
                                // starting to speak. Discard it, then open the
                                // session that will outlive the key.
                                let _ = tx.send(Input::Cancelled { at });
                                let _ = tx.send(pressed(&e));
                            }
                            OnPress::StopLatched => {
                                e.shell.set_latched(false);
                                let _ = tx.send(Input::HotkeyReleased { at });
                            }
                        }
                    }
                    HotkeyEvent::Pressed => {
                        let _ = tx.send(pressed(&e));
                    }
                    HotkeyEvent::Released if push_to_talk => {
                        // Swallowed when it belongs to the press that latched the
                        // microphone open, or to the tap that closed one.
                        if e.latch.lock().expect("latch").release(at.0) == OnRelease::Stop {
                            let _ = tx.send(Input::HotkeyReleased { at });
                        }
                    }
                    HotkeyEvent::Released => {
                        let _ = tx.send(Input::HotkeyReleased { at });
                    }
                    HotkeyEvent::Cancelled => {
                        // Before the session work, and unconditional: Escape
                        // means "back out of whatever this is", and when nothing
                        // is recording the thing to back out of is the open menu.
                        e.shell.on_cancel_key();
                        e.latch.lock().expect("latch").forget();
                        e.shell.set_latched(false);
                        let _ = tx.send(Input::Cancelled { at });
                    }
                }
            }))
            .map_err(|e| e.to_string())?;
    }

    // Drives the maximum-recording cutoff so a stuck key cannot record forever.
    {
        let tx = tx.clone();
        let e = engine.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if tx.send(Input::Tick { at: e.now() }).is_err() {
                break;
            }
        });
    }

    {
        let e = engine.clone();
        let tx2 = tx.clone();
        let limits = config.limits;
        let activation = config.activation;
        std::thread::spawn(move || {
            // The user's own limits and activation style, not the defaults.
            //
            // This was `SessionLimits::default()`, which quietly discarded the
            // whole `[limits]` section: the "Maximum recording" setting in the UI
            // wrote a value to disk that nothing ever read, so choosing 5 minutes
            // still cut the recording off at 2.
            let mut machine = SessionMachine::with_activation(limits, activation);
            for input in rx {
                for effect in machine.handle(input) {
                    execute(&e, &tx2, effect);
                }
            }
        });
    }

    Ok((engine, ready))
}

/// Perform one effect. Anything slow moves to a worker thread so the machine loop
/// keeps responding — a blocking decode here would make Escape unusable during the
/// half-second when the user most wants it.
fn execute(e: &Arc<Engine>, tx: &Sender<Input>, effect: Effect) {
    match effect {
        Effect::StartCapture { .. } => {
            let e2 = e.clone();

            // Throttle to ~30 Hz.
            //
            // The audio callback runs at the WASAPI buffer rate — around 100 times
            // a second — and forwarding every frame meant 100 IPC messages and 100
            // React renders per second. The waveform visibly flickered, which is
            // the opposite of what a status indicator should do.
            //
            // 30 Hz matches the rate the waveform samples at, so nothing is lost:
            // the extra frames were being interpolated over anyway.
            let last = Mutex::new(Instant::now() - Duration::from_millis(100));
            let levels: Arc<dyn Fn(LevelFrame) + Send + Sync> = Arc::new(move |f| {
                {
                    let mut t = last.lock().expect("level clock");
                    if t.elapsed() < Duration::from_millis(33) {
                        return;
                    }
                    *t = Instant::now();
                }
                e2.shell.emit(&Event::Level {
                    rms: f.rms,
                    peak: f.peak,
                    elapsed_ms: 0,
                });
            });
            if let Err(err) = e.audio.start(levels) {
                tracing::error!(error = %err, "capture failed to start");
                e.shell.emit(&Event::Notice {
                    level: ov_core::event::NoticeLevel::Error,
                    message: format!("Microphone unavailable: {err}"),
                });
            }
        }

        Effect::StopCapture { session } => {
            let e = e.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let at = e.now();
                match e.audio.stop() {
                    Ok(pcm) => {
                        let duration_ms = pcm.duration_ms();
                        let rms = pcm.rms();
                        e.captured
                            .lock()
                            .expect("capture mutex")
                            .insert(session, pcm);
                        let _ = tx.send(Input::AudioCaptured {
                            session,
                            duration_ms,
                            rms,
                            at,
                        });
                    }
                    Err(err) => {
                        let _ = tx.send(Input::AudioFailed {
                            session,
                            error: err.to_string(),
                            at,
                        });
                    }
                }
            });
        }

        // Cancelling drains the whole machine — the live capture and every queued
        // session — so every buffer goes with it.
        Effect::AbortCapture { .. } => {
            let _ = e.audio.abort();
            e.captured.lock().expect("capture mutex").clear();
        }

        Effect::Transcribe { session } => {
            let e = e.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let audio = e.captured.lock().expect("capture mutex").remove(&session);
                let Some(audio) = audio else {
                    let _ = tx.send(Input::TranscriptionFailed {
                        session,
                        error: "audio buffer was empty".into(),
                        at: e.now(),
                    });
                    return;
                };
                // Proper nouns only -- never the whole dictionary.
                //
                // Filling this with identifiers was measured to make output
                // worse (see ov-format's dictionary module docs), and that
                // finding stands. It does not extend to names that are ordinary
                // words phonetically: "Claude" is indistinguishable from "cloud"
                // once the audio is gone, so the decoder has to be told it is a
                // candidate while it still has it. `hint_terms` is the short,
                // explicitly-marked subset. Language is not a hint in this sense
                // -- it is the user's own setting, read once at startup.
                let hint = DecodeHint {
                    vocabulary: e.rules.lock().expect("rules").hints.clone(),
                    language: e.language.lock().expect("language mutex").clone(),
                };
                match e.transcriber.transcribe(&audio, &hint) {
                    Ok(transcript) => {
                        let _ = tx.send(Input::Transcribed {
                            session,
                            transcript,
                            at: e.now(),
                        });
                    }
                    Err(err) => {
                        let _ = tx.send(Input::TranscriptionFailed {
                            session,
                            error: err.to_string(),
                            at: e.now(),
                        });
                    }
                }
            });
        }

        Effect::Format {
            session,
            raw,
            profile,
        } => {
            let text = e.format(&profile, &raw);
            let _ = tx.send(Input::Formatted {
                session,
                text,
                at: e.now(),
            });
        }

        Effect::Inject {
            session,
            text,
            target_exe,
        } => {
            let e = e.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                // Transcription and formatting can take several seconds (multi-second
                // decodes are routine), so focus may have moved on from whichever app
                // was foreground when the user started speaking. Nothing here can
                // stop a paste from landing in the wrong window if that happened, but
                // logging the mismatch is the difference between "injection sometimes
                // silently fails in one app" being a mystery and being a two-minute
                // diagnosis next time it happens.
                let now_exe = e.apps.foreground().unwrap_or_default().exe;
                if !target_exe.is_empty() && now_exe != target_exe {
                    tracing::warn!(
                        pressed_in = %target_exe,
                        injecting_into = %now_exe,
                        "foreground app changed between press and injection"
                    );
                }

                let mode = ov_input::mode_for(&text, e.paste_threshold);
                match e.sink.inject(&text, mode) {
                    Ok(_) => {
                        *e.last_text.lock().expect("last text mutex") = text;
                        let _ = tx.send(Input::Injected {
                            session,
                            at: e.now(),
                        });
                    }
                    Err(err) => {
                        // Not lost: the injector leaves the text on the clipboard. This
                        // used to be invisible in the log entirely -- the only trace
                        // was an in-app notice and a history badge, neither of which
                        // helps once the moment has passed.
                        tracing::warn!(
                            error = %err,
                            target = %now_exe,
                            "injection failed; text left on the clipboard"
                        );
                        *e.last_text.lock().expect("last text mutex") = text;
                        let _ = tx.send(Input::InjectionFailed {
                            session,
                            error: err.to_string(),
                            at: e.now(),
                        });
                    }
                }
            });
        }

        Effect::Persist { record } => {
            // Redaction happens here, on the way to storage, and nowhere else.
            //
            // The transcript reached the user's application several steps ago —
            // `Effect::Inject` ran before this — so what is being protected is the
            // *retained* copy: the searchable history and anything a log ends up
            // in. Redacting before injection would mean silently handing the user
            // `[redacted]` instead of the words they said, which is a data-loss
            // bug however good the intention.
            let redactor = e.redactor.lock().expect("redactor mutex");
            let (raw_text, final_text) = if redactor.is_empty() {
                (record.raw_text.clone(), record.final_text.clone())
            } else {
                (
                    redactor.apply(&record.raw_text),
                    redactor.apply(&record.final_text),
                )
            };
            drop(redactor);

            let entry = Utterance {
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
                duration_ms: record.audio_ms,
                raw_text,
                final_text,
                profile: record.profile.clone(),
                target_app: (!record.app.exe.is_empty()).then(|| record.app.exe.clone()),
                model: e.transcriber.model_id(),
                status: record.outcome.code().to_string(),
                latency_ms: record.latency_ms,
            };
            // A failed history write must never cost the user their text — it is
            // already in the target application by this point.
            if let Err(err) = e.history.append(&entry) {
                tracing::warn!(error = %err, "history write failed");
            }
            if matches!(
                record.outcome,
                Outcome::Delivered | Outcome::ClipboardFallback(_)
            ) {
                *e.last_text.lock().expect("last text mutex") = record.final_text.clone();
            }
        }

        Effect::Emit(event) => {
            // The overlay follows the session, not the user: it appears when a
            // session starts and hides when everything is finished.
            match &event {
                Event::Listening { .. } => e.shell.set_overlay_visible(true),
                Event::Idle => e.shell.set_overlay_visible(false),
                _ => {}
            }
            e.shell.emit(&event);
        }
    }
}
