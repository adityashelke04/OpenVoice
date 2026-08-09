//! The composition root.
//!
//! Owns every adapter and drives the pure state machine, exactly as `ov-cli` does.
//! The only difference is where effects go: the CLI prints them, this emits them to
//! the webview. `ov-core` does not know either exists, which is the entire point of
//! the port boundary — and it is why the CLI still works as a headless test harness
//! for the same pipeline.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ov_core::config::SessionLimits;
use ov_core::event::Event;
use ov_core::ports::{
    AppContext, AudioSource, DecodeHint, HistoryStore, HotkeyEvent, HotkeyListener, LevelFrame,
    Pcm16k, TextSink, Transcriber, Utterance,
};
use ov_core::session::{Effect, Input, SessionMachine};
use ov_core::types::{Millis, Outcome};
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

/// How far the first-run model download has got.
///
/// `total` is 0 when the size could not be determined ahead of time; show an
/// indeterminate bar rather than 0%.
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
    /// Record download progress, or `None` once there is nothing downloading.
    ///
    /// Recorded rather than emitted. A first run downloads over a gigabyte before
    /// the window has finished loading, so an event would be published to nobody;
    /// the UI polls status instead, and cannot miss what it asks for.
    fn set_download_progress(&self, progress: Option<DownloadProgress>);
}

/// Profiles and their compiled formatters, replaced as a unit.
///
/// Kept together because they must never disagree: a formatter compiled from a
/// profile that has since been edited would format with stale rules while the app
/// reports the new ones.
struct Rules {
    profiles: Vec<Profile>,
    formatters: Vec<(String, Formatter)>,
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
        Self {
            profiles,
            formatters,
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
    transcriber: ov_asr::SidecarTranscriber,
    sink: ov_input::WinTextSink,
    apps: ov_input::WinForeground,
    /// Profiles and formatters live behind one lock and are replaced together.
    /// Rebuilding them live is what makes "changes apply to the next thing you
    /// dictate" true rather than a claim that quietly requires a restart.
    rules: Mutex<Rules>,
    captured: Mutex<Option<Pcm16k>>,
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
    /// Unlike the model, this needs no restart: it is a per-request parameter
    /// faster-whisper reads at decode time, not something baked into loaded
    /// weights, so it reloads the same way the dictionary and profiles do.
    language: Mutex<Option<String>>,
    start: Instant,
    shell: Arc<dyn Shell>,
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

/// Locate the repository root, verifying each candidate rather than guessing.
/// See the same function in `ov-cli` for why this is not a simple parent walk.
fn find_root() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("OPENVOICE_ROOT") {
        candidates.push(PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.to_path_buf());
        }
    }
    if let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2) {
        candidates.push(root.to_path_buf());
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.extend(cwd.ancestors().map(Path::to_path_buf));
    }
    candidates
        .into_iter()
        .find(|p| p.join("sidecar").join("openvoice_asr").is_dir())
}

/// Locate an interpreter that has faster-whisper installed.
///
/// Only *conventions* are searched — an override, an activated virtualenv, and
/// the two repo-local paths the documented setup commands create. Never a
/// specific machine: an absolute path to one developer's environment baked into
/// a public binary is a privacy leak and useless to everyone else. If the
/// interpreter lives somewhere unusual, point at it explicitly:
///
/// ```powershell
/// setx OPENVOICE_PYTHON D:\dev\openvoice-venv\Scripts\python.exe
/// ```
fn find_python(root: &Path) -> Option<PathBuf> {
    let mut c: Vec<PathBuf> = Vec::new();

    if let Some(p) = std::env::var_os("OPENVOICE_PYTHON") {
        c.push(PathBuf::from(p));
    }
    if let Some(venv) = std::env::var_os("VIRTUAL_ENV") {
        let venv = PathBuf::from(venv);
        c.push(venv.join("Scripts/python.exe"));
        c.push(venv.join("bin/python"));
    }
    for base in [root.join(".venv"), root.join("sidecar/.venv")] {
        c.push(base.join("Scripts/python.exe"));
        c.push(base.join("bin/python"));
    }

    c.into_iter().find(|p| p.is_file())
}

/// Locate the frozen sidecar shipped inside an installed copy.
///
/// `resource_dir` is what Tauri reports; the two fallbacks cover a build that was
/// run in place rather than installed, where the resource directory sits beside
/// the executable instead of under it.
fn find_bundled(resource_dir: Option<&Path>) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = resource_dir {
        candidates.push(dir.join("sidecar/openvoice-asr.exe"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("resources/sidecar/openvoice-asr.exe"));
            candidates.push(dir.join("sidecar/openvoice-asr.exe"));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Build a configuration that runs the sidecar from a repository checkout.
fn from_checkout(model: &str) -> Result<ov_asr::SidecarConfig, String> {
    let root = find_root().ok_or_else(|| {
        "Could not find the OpenVoice sources. Set OPENVOICE_ROOT to a checkout.".to_string()
    })?;
    let python = find_python(&root).ok_or_else(|| {
        format!(
            "Found the OpenVoice sources at {} but no Python environment with \
             faster-whisper installed. Create one with `uv sync` in `sidecar/`, or \
             set OPENVOICE_PYTHON to an interpreter that has it.",
            root.display()
        )
    })?;
    Ok(ov_asr::SidecarConfig::dev(&root, python, model))
}

/// Decide how the speech engine is going to be started.
///
/// Both routes are always available as a fallback; only the *order* changes,
/// and it changes on who is running the binary rather than on what is on disk:
///
/// * A **release build** prefers the frozen engine it was packaged with. An
///   installed copy must never end up depending on a repository that happens to
///   be lying around — that works right up until the user moves the folder, and
///   then the app breaks for no visible reason.
/// * A **debug build**, or one where `OPENVOICE_ROOT`/`OPENVOICE_PYTHON` is set,
///   prefers the checkout. Tauri stages bundle resources into the target
///   directory during development too, so preferring the bundle everywhere would
///   quietly run a developer's *last frozen* sidecar instead of the Python they
///   are editing — and the edits would appear to do nothing.
fn locate_sidecar(
    resource_dir: Option<&Path>,
    model: &str,
) -> Result<ov_asr::SidecarConfig, String> {
    let prefer_checkout = cfg!(debug_assertions)
        || std::env::var_os("OPENVOICE_ROOT").is_some()
        || std::env::var_os("OPENVOICE_PYTHON").is_some();

    if prefer_checkout {
        match from_checkout(model) {
            Ok(cfg) => {
                tracing::info!("using the speech engine from a checkout");
                return Ok(cfg);
            }
            Err(e) => tracing::debug!(reason = %e, "no usable checkout; trying the bundle"),
        }
    }

    if let Some(exe) = find_bundled(resource_dir) {
        tracing::info!(path = %exe.display(), "using the bundled speech engine");
        return Ok(ov_asr::SidecarConfig::bundled(exe, model));
    }

    // Nothing worked. Report the checkout's own diagnosis rather than a generic
    // message: it names the directory it looked in, which is the one fact that
    // makes this fixable.
    from_checkout(model).map_err(|e| {
        format!("{e} This build also has no speech engine bundled with it — reinstall the app.")
    })
}

/// Build every adapter, start the hotkey hook, and run the event loop on a
/// background thread. Returns once the model is warm.
///
/// `resource_dir` is Tauri's resource directory, or `None` when the caller has
/// none. It is passed in rather than resolved here so this module stays free of
/// Tauri types and testable on its own.
pub fn start(
    shell: Arc<dyn Shell>,
    settings: &crate::settings::Settings,
    history: Arc<dyn HistoryStore>,
    resource_dir: Option<&Path>,
) -> Result<(Arc<Engine>, Ready), String> {
    let config = settings.config.clone();
    let model = settings.model.as_str();

    let mut cfg = locate_sidecar(resource_dir, model)?;
    let data = crate::history::data_dir();
    // Weights and CUDA libraries live under the app's own data directory, so that
    // uninstalling reclaims the several gigabytes they occupy. A developer's
    // shared HF cache is left alone: `dev` leaves both as `None`, and only an
    // installed copy sets them.
    if cfg.package_dir.is_none() {
        cfg.model_dir = Some(data.join("models"));
        let cuda = data.join("cuda");
        cfg.cuda_dir = cuda.is_dir().then_some(cuda);
    }
    // Backs the "Keep recordings" toggle, which until now was a switch with
    // nothing behind it. Off means the scratch WAV is deleted after every decode,
    // exactly as before; on means it is moved here instead.
    cfg.retain_audio_dir = config
        .privacy
        .retain_audio
        .then(|| crate::history::data_dir().join("audio"));
    if let Some(dir) = &cfg.retain_audio_dir {
        tracing::warn!(dir = %dir.display(), "keeping recordings on disk; this is off by default");
    }
    cfg.allow_download = std::env::var("OPENVOICE_ALLOW_DOWNLOAD").is_ok();
    let transcriber = ov_asr::SidecarTranscriber::new(cfg).map_err(|e| e.to_string())?;

    // Fetch the weights before warming. `warm` on a model that is not present
    // fails with a message about a missing repository, which tells a first-run
    // user nothing; this reports a download they can watch instead.
    //
    // Cheap and silent when the model is already cached, which is every run after
    // the first.
    let name = model.to_string();
    let reporter = shell.clone();
    let downloaded = transcriber
        .ensure_model(|p| {
            reporter.set_download_progress(Some(DownloadProgress {
                model: name.clone(),
                done: p.done,
                total: p.total,
            }));
        })
        .map_err(|e| format!("Could not get the {model} model: {e}"))?;
    // Cleared unconditionally: leaving a completed download in place would keep
    // the UI on the progress screen for the whole of the model load that follows.
    shell.set_download_progress(None);
    if downloaded {
        tracing::info!(model, "downloaded model weights");
    }

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
        model: model.to_string(),
        device: transcriber.model_id(),
        shortcut: "Right Ctrl".into(),
        mic,
    };

    let engine = Arc::new(Engine {
        audio,
        transcriber,
        sink: ov_input::WinTextSink::new(config.paste_threshold_chars),
        apps: ov_input::WinForeground,
        rules: Mutex::new(rules),
        captured: Mutex::new(None),
        history,
        last_text: Mutex::new(String::new()),
        redactor: Mutex::new(redactor),
        paste_threshold: config.paste_threshold_chars,
        language: Mutex::new(config.language.clone()),
        start: Instant::now(),
        shell,
    });

    let (tx, rx) = channel::<Input>();

    // Hotkey -> machine input. The foreground application is sampled on *press*:
    // by injection time the user may have switched windows, and the profile must
    // reflect where they were speaking.
    let hotkey = ov_input::WinHotkeyListener::new(config.chord);
    {
        let tx = tx.clone();
        let e = engine.clone();
        hotkey
            .start(Arc::new(move |event| {
                let at = e.now();
                let input = match event {
                    HotkeyEvent::Pressed => {
                        let app = e.apps.foreground().unwrap_or_default();
                        let profile = e.profile_for(&app.exe);
                        Input::HotkeyPressed { at, app, profile }
                    }
                    HotkeyEvent::Released => Input::HotkeyReleased { at },
                    HotkeyEvent::Cancelled => Input::Cancelled { at },
                };
                let _ = tx.send(input);
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
        std::thread::spawn(move || {
            let mut machine = SessionMachine::new(SessionLimits::default());
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
                        *e.captured.lock().expect("capture mutex") = Some(pcm);
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

        Effect::AbortCapture { .. } => {
            let _ = e.audio.abort();
            *e.captured.lock().expect("capture mutex") = None;
        }

        Effect::Transcribe { session } => {
            let e = e.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let audio = e.captured.lock().expect("capture mutex").take();
                let Some(audio) = audio else {
                    let _ = tx.send(Input::TranscriptionFailed {
                        session,
                        error: "audio buffer was empty".into(),
                        at: e.now(),
                    });
                    return;
                };
                // Vocabulary hints are deliberately left empty: measured to make
                // output worse. See ov-format's dictionary module docs. Language is
                // not a hint in that sense -- it is the user's own setting, read
                // once at startup (see `language` field docs above).
                let hint = DecodeHint {
                    vocabulary: vec![],
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
