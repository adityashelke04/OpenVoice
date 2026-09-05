//! # OpenVoice — desktop application
//!
//! The Tauri shell. It owns the windows, the tray, and the bridge between
//! `ov-core`'s event stream and the webview. All product logic lives in the
//! engine crates; this file is wiring.
//!
//! Two things here are load-bearing and easy to break:
//!
//! 1. **The overlay must never take focus.** `WS_EX_NOACTIVATE` is applied to it
//!    after creation. Without that flag, showing the overlay steals focus from the
//!    user's editor, the caret position is lost, and injection lands in the wrong
//!    place — which is the whole product failing.
//! 2. **Closing the Hub hides it; it does not quit.** A dictation tool that stops
//!    working when you close its window is not available when you need it.

// The window must exist before the tray can reference it; the console subsystem is
// disabled so a stray terminal never appears behind the app in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![warn(clippy::all)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ov_core::event::Event;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

mod clickaway;
mod engine;
mod history;
mod models;
mod overlay;
mod settings;
mod taplatch;
mod topmost;
mod update;

/// The Tauri channel every domain event is published on.
const EVENT: &str = "ov://event";

/// Adapts the Tauri app handle to the engine's `Shell` port, so the engine never
/// learns that a GUI exists.
struct TauriShell {
    app: AppHandle,
}

impl engine::Shell for TauriShell {
    fn emit(&self, event: &Event) {
        let _ = self.app.emit(EVENT, event);
    }

    fn set_overlay_visible(&self, active: bool) {
        let Some(win) = overlay::window(&self.app) else {
            tracing::error!("overlay window is missing");
            return;
        };
        self.app
            .state::<AppState>()
            .overlay
            .set_active(&win, active);
    }

    /// Told rather than polled, like `overlay-mini` and `overlay-auto-collapse`.
    ///
    /// A missing window is not an error worth logging here: the latch is cleared
    /// on every route out of a session, including ones that can run while the
    /// bar is snoozed or the app is shutting down, and a warning on each of
    /// those would be noise about nothing.
    fn set_latched(&self, latched: bool) {
        if let Some(win) = overlay::window(&self.app) {
            let _ = win.emit("overlay-latched", latched);
        }
    }
}

/// Engine lifecycle as the UI sees it.
///
/// Polled rather than pushed. `ov://ready` and `ov://error` are one-shot events, and
/// the engine can finish — or fail — before the webview has attached its listener.
/// That race left the UI reporting "Starting…" forever with the real error emitted
/// into the void, which is the worst possible failure for something a person is
/// trying to debug. A status the UI can *ask* for cannot be missed.
#[derive(Clone, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum Status {
    Starting,
    Ready(engine::Ready),
    Failed { error: String },
}

#[tauri::command]
fn get_status(state: tauri::State<'_, AppState>) -> Status {
    // A failure outranks a success: an engine that came up and then died must not
    // still report Ready.
    if let Some(e) = state.error.lock().expect("error").clone() {
        return Status::Failed { error: e };
    }
    if let Some(r) = state.ready.lock().expect("ready").clone() {
        return Status::Ready(r);
    }
    Status::Starting
}

/// Everything the UI asks for once, at startup.
#[tauri::command]
fn get_ready(state: tauri::State<'_, AppState>) -> Option<engine::Ready> {
    state.ready.lock().expect("ready").clone()
}

/// Where the log file lives, so the UI can point at it when something breaks.
#[tauri::command]
fn get_log_path() -> String {
    history::data_dir()
        .join("openvoice.log")
        .to_string_lossy()
        .into()
}

/// A history row as the UI consumes it.
///
/// Deliberately not `ov_core::ports::Utterance` re-exported: that type is a domain
/// value, and letting the interface bind to it would make an internal rename a
/// frontend breakage. This is a wire format, and it is allowed to be boring.
#[derive(serde::Serialize)]
struct HistoryRow {
    created_at: u64,
    outcome: String,
    raw_text: String,
    final_text: String,
    profile: String,
    target_app: String,
    audio_ms: u64,
    latency_ms: u64,
}

impl From<ov_core::ports::Utterance> for HistoryRow {
    fn from(u: ov_core::ports::Utterance) -> Self {
        Self {
            created_at: u.created_at,
            outcome: u.status,
            raw_text: u.raw_text,
            final_text: u.final_text,
            profile: u.profile,
            target_app: u.target_app.unwrap_or_default(),
            audio_ms: u.duration_ms,
            latency_ms: u.latency_ms,
        }
    }
}

/// Recent history, or full-text search when `query` is given.
///
/// Search happens in SQLite rather than by filtering in JavaScript: the previous
/// version fetched 200 rows and matched them client-side, so anything older than
/// the last 200 sessions was simply unfindable.
#[tauri::command]
fn get_history(
    state: tauri::State<'_, AppState>,
    limit: Option<usize>,
    query: Option<String>,
) -> Vec<HistoryRow> {
    use ov_core::ports::HistoryStore;
    let limit = limit.unwrap_or(200);
    let store = &state.store;

    let rows = match query.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => store.search(q, limit),
        None => store.recent(limit),
    };

    rows.unwrap_or_else(|e| {
        tracing::warn!(error = %e, "history read failed");
        Vec::new()
    })
    .into_iter()
    .map(HistoryRow::from)
    .collect()
}

/// Aggregate figures, computed in SQL over the whole history.
///
/// The UI used to sum a page of rows in JavaScript, which silently ignored
/// everything older than the page and got the word count wrong as soon as history
/// exceeded 200 sessions.
#[tauri::command]
fn get_totals(state: tauri::State<'_, AppState>) -> serde_json::Value {
    let totals = state.store.totals().unwrap_or_default();
    let top = state.store.top_app().ok().flatten();
    // Raw timestamps, not a streak: the streak depends on the user's timezone, and
    // the database has no business guessing it.
    let days = state.store.active_days(400).unwrap_or_default();

    serde_json::json!({
        "sessions": totals.sessions,
        "words": totals.words,
        "speakingMs": totals.speaking_ms,
        "topApp": top.map(|(name, count)| serde_json::json!({ "name": name, "count": count })),
        "activeDays": days,
    })
}

/// Delete all history. Backs the "panic purge" the privacy design calls for.
#[tauri::command]
fn clear_history(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.store.clear().map_err(|e| e.to_string())
}

#[tauri::command]
fn paste_last(state: tauri::State<'_, AppState>) {
    if let Some(e) = state.engine.lock().expect("engine").as_ref() {
        e.paste_last();
    }
}

#[tauri::command]
fn open_data_dir(app: AppHandle) {
    let dir = history::data_dir();
    let _ =
        tauri_plugin_opener::OpenerExt::opener(&app).open_path(dir.to_string_lossy(), None::<&str>);
}

struct AppState {
    engine: Mutex<Option<Arc<engine::Engine>>>,
    ready: Mutex<Option<engine::Ready>>,
    error: Mutex<Option<String>>,
    /// Whether a start attempt is in flight, so a retry cannot begin a second one
    /// beside it. Two live engines would mean two sidecars and two hotkey hooks.
    starting: AtomicBool,
    /// Set while a model is being fetched from the Models screen.
    ///
    /// Recorded rather than emitted as an event: a 465 MB transfer can start
    /// before the webview has finished loading, so an event would be published
    /// to nobody. The screen polls, and polling cannot miss what it asks for.
    download: Mutex<Option<engine::DownloadProgress>>,
    /// The settings the running engine was actually built from.
    ///
    /// Kept so "does this need a restart?" can be *answered* rather than
    /// hardcoded: the question is only ever whether the live engine differs from
    /// what is now on disk, and a snapshot of what it booted with is the one
    /// source that cannot drift as settings are added.
    booted: Mutex<Option<settings::Settings>>,
    overlay: overlay::Overlay,
    settings: settings::Store,
    store: Arc<ov_store::SqliteStore>,
}

impl Default for AppState {
    fn default() -> Self {
        let settings = settings::Store::load();
        Self {
            engine: Mutex::new(None),
            ready: Mutex::new(None),
            error: Mutex::new(None),
            // The launch attempt begins immediately, so this starts true.
            starting: AtomicBool::new(true),
            download: Mutex::new(None),
            booted: Mutex::new(None),
            overlay: overlay::Overlay::new(),
            store: open_history(&settings),
            settings,
        }
    }
}

/// Open the history database, importing anything the old JSONL file still holds.
///
/// If SQLite cannot be opened the app still starts, on an in-memory database.
/// Dictation working without a saved transcript is a far better failure than an
/// app that refuses to launch because a file is locked.
fn open_history(settings: &settings::Store) -> Arc<ov_store::SqliteStore> {
    use ov_core::ports::HistoryStore;

    let dir = history::data_dir();
    let store = ov_store::SqliteStore::open(dir.join("history.db")).unwrap_or_else(|e| {
        tracing::error!(error = %e, "history database unavailable; using memory only");
        ov_store::SqliteStore::open(":memory:").expect("in-memory database")
    });

    // Existing users have real transcripts in the file this replaces. Shipping
    // without importing them would reset their word count and streak to zero,
    // which from their side is indistinguishable from losing the data.
    match ov_store::import_jsonl(&store, dir.join("history.jsonl")) {
        Ok(0) => {}
        Ok(n) => tracing::info!(imported = n, "migrated history from the previous format"),
        Err(e) => tracing::warn!(error = %e, "could not import the old history file"),
    }

    // Apply the retention setting once at startup rather than on a timer: history
    // only grows while the app is running, so a daily check on launch is enough.
    let privacy = settings.get().config.privacy;
    if let Err(e) = store.purge_older_than(privacy.history_days) {
        tracing::warn!(error = %e, "retention purge failed");
    }

    // Recordings are swept on the same schedule and are the reason a sweep
    // matters at all: history is a few hundred bytes an utterance, audio is a few
    // hundred kilobytes. Run unconditionally rather than only when `retain_audio`
    // is on, so turning the setting *off* also clears out what it left behind
    // instead of stranding it forever.
    let audio_dir = history::data_dir().join("audio");
    if let Err(e) = ov_asr::recordings::purge_recordings(&audio_dir, privacy.audio_days) {
        tracing::warn!(error = %e, "recording purge failed");
    }

    Arc::new(store)
}

/* -- settings commands ------------------------------------------------------ */

#[tauri::command]
fn get_settings(state: tauri::State<'_, AppState>) -> settings::Settings {
    state.settings.get()
}

/// Persist a whole settings document.
///
/// The UI sends the complete document rather than a patch. For a single-user
/// desktop app that is simpler and has no lost-update problem worth the extra
/// machinery, and the store validates before it writes.
#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    settings: settings::Settings,
) -> Result<settings::Settings, String> {
    let before = state.settings.get();
    let saved = state.settings.update(|s| *s = settings)?;

    // Push the edited dictionary into the running engine so corrections take effect
    // on the very next utterance. Without this the Dictionary screen would quietly
    // require a restart, and the interface says otherwise.
    //
    // The shortcut is here for exactly the same reason, and was the one thing this
    // function persisted without ever applying: the keyboard hook kept the key it
    // was handed at launch, so a rebind took effect only on the next start while
    // the Settings screen showed it as already in force.
    if let Some(e) = state.engine.lock().expect("engine").as_ref() {
        e.reload_rules(&saved);
        e.reload_language(&saved);

        if before.config.chord != saved.config.chord
            || before.config.activation != saved.config.activation
        {
            if let Err(err) = e.reload_hotkey(&saved) {
                // Not fatal to the save -- the setting is on disk and will hold
                // from the next launch -- but the user must be told, because the
                // failure is invisible: they would press the new key and get
                // nothing, with the screen insisting it is bound.
                tracing::error!(error = %err, "shortcut could not be applied live");
                return Err(format!(
                    "Saved, but the new shortcut could not be applied while it is running ({err}). Restart OpenVoice to start using it."
                ));
            }
            publish_shortcut(&app, &state, &saved);
        }
    }

    Ok(saved)
}

/// Tell the rest of the app which key is bound now.
///
/// The Hub's instructions and the Flow Bar's idle pill both name the shortcut, and
/// both read it from the one-shot `Ready` payload built at launch. Rebinding
/// without republishing would leave every on-screen mention of the key pointing at
/// the old one -- which is worse than the original bug, because the app would then
/// be telling the user to press a key that genuinely no longer works.
fn publish_shortcut(
    app: &AppHandle,
    state: &tauri::State<'_, AppState>,
    saved: &settings::Settings,
) {
    let mut guard = state.ready.lock().expect("ready");
    let Some(ready) = guard.as_mut() else { return };
    ready.shortcut = saved.config.chord.key.label().into();
    let updated = ready.clone();
    drop(guard);
    let _ = app.emit("ov://ready", updated);
}

/// What is saved but not yet in force, in the user's words.
///
/// Answered by diffing the live engine's boot settings against what is on disk,
/// and listing only the fields that genuinely cannot be applied to a running
/// engine. The shortcut, activation style, language, dictionary and profiles are
/// deliberately absent: they reload in place, and naming them here would train the
/// user to restart for changes that already took effect.
#[tauri::command]
fn restart_reasons(app: AppHandle, state: tauri::State<'_, AppState>) -> Vec<String> {
    let Some(booted) = state.booted.lock().expect("booted").clone() else {
        return Vec::new();
    };
    // Through `effective_settings`, not the store, so the comparison is against
    // what a restart would *actually* load. With `OPENVOICE_MODEL` set the two
    // differ permanently, and reading the store here would pin the banner open on
    // a model change that restarting could never deliver.
    let now = effective_settings(&app);
    let mut reasons = Vec::new();

    // Weights are loaded once, at warm-up, so choosing a different model on the
    // Models screen takes effect at the next start and not before.
    if booted.model != now.model {
        reasons.push("the speech model".to_string());
    }
    // The capture device is opened when the audio source is built.
    if booted.config.input_device != now.config.input_device {
        reasons.push("the microphone".to_string());
    }
    // Limits are moved into the session machine when its thread is spawned.
    if booted.config.limits != now.config.limits {
        reasons.push("the maximum recording length".to_string());
    }
    // Decided once, when the recording store is opened.
    if booted.config.privacy.retain_audio != now.config.privacy.retain_audio {
        reasons.push("keeping recordings".to_string());
    }

    reasons
}

/// Ask whether a newer version exists, right now.
///
/// Reached from the "Check now" button, which is why the error is returned rather
/// than logged: the user is waiting for an answer, and silence would read as "you
/// are up to date" — the one wrong answer they cannot tell apart from the truth.
#[tauri::command]
async fn check_for_update(app: AppHandle) -> Result<update::UpdateStatus, String> {
    update::check(&app).await
}

/// Download, verify and apply an update, then restart.
///
/// A separate command from [`check_for_update`] on purpose. Nothing is downloaded
/// as a side effect of finding out that something exists.
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    update::install(&app).await
}

/// The settings the engine should start with.
///
/// This used to apply an `OPENVOICE_MODEL` override, which was the escape hatch
/// for a configuration that had pinned itself to a model that would not load.
/// With one model that cannot be selected, there is nothing to override and no
/// such corner to be stuck in.
fn effective_settings(app: &AppHandle) -> settings::Settings {
    app.state::<AppState>().settings.get()
}

/// Start the engine on a background thread, recording the outcome in state.
///
/// Called at launch and again by [`retry_engine`]. It is one function rather than
/// two because a retry that differed from the original start in any way would be
/// a second code path that only ever runs on the machines least able to report
/// what it did.
fn spawn_engine(app: AppHandle) {
    std::thread::spawn(move || {
        let shell = Arc::new(TauriShell { app: app.clone() });
        let settings = effective_settings(&app);
        let store = app.state::<AppState>().store.clone();
        // Where the frozen speech engine lives in an installed copy. `Err` simply
        // means this is not one, and the engine falls back to a repository checkout.
        match engine::start(shell, &settings, store) {
            Ok((engine, ready)) => {
                let state = app.state::<AppState>();
                *state.engine.lock().expect("engine") = Some(engine);
                *state.ready.lock().expect("ready") = Some(ready.clone());
                *state.error.lock().expect("error") = None;
                // Snapshot what this engine was actually built from, so
                // `restart_reasons` compares against reality rather than against
                // whatever happens to be on disk later.
                *state.booted.lock().expect("booted") = Some(settings.clone());
                let _ = app.emit("ov://ready", ready);
                tracing::info!("engine ready");
            }
            Err(e) => {
                tracing::error!(error = %e, "engine failed to start");
                // Recorded in state as well as emitted: the event can be missed,
                // the state cannot.
                *app.state::<AppState>().error.lock().expect("error") = Some(e.clone());
                let _ = app.emit("ov://error", e);
            }
        }
        app.state::<AppState>()
            .starting
            .store(false, Ordering::SeqCst);
    });
}

/// Try starting the engine again after a failure.
///
/// The startup path fetches weights and then loads them, and either half can fail
/// for reasons that have nothing to do with this machine — a dropped connection,
/// a rate-limited request to a public model host. Until now that failure was
/// permanent in the only sense that matters to a user: the app recorded the error
/// and offered nothing to do about it, and relaunching simply reproduced it.
///
/// Returns whether a fresh attempt was started. `false` means one is already
/// running, which is not an error — it is the answer to someone pressing the
/// button twice.
#[tauri::command]
fn retry_engine(app: AppHandle, state: tauri::State<'_, AppState>) -> bool {
    if state.engine.lock().expect("engine").is_some() {
        return false;
    }
    // `swap` rather than load-then-store: two clicks landing together must not
    // both start an engine, and each would hold a sidecar and a hotkey hook.
    if state.starting.swap(true, Ordering::SeqCst) {
        return false;
    }
    *state.error.lock().expect("error") = None;
    tracing::info!("retrying engine start");
    spawn_engine(app);
    true
}

/// Input devices, for the microphone picker.
#[tauri::command]
fn list_microphones() -> Vec<String> {
    use ov_core::ports::AudioSource;
    ov_audio::CpalAudioSource::new(None)
        .and_then(|a| a.devices())
        .unwrap_or_default()
}

/// Preview what the formatter does to a phrase, using the live dictionary.
///
/// This is what makes the Dictionary screen teachable: you type what the model
/// heard, and see what OpenVoice would write, without dictating anything.
#[tauri::command]
fn preview_format(
    state: tauri::State<'_, AppState>,
    text: String,
    profile: String,
) -> Vec<(String, String)> {
    use ov_format::profile::Profile;

    let s = state.settings.get();
    let mut entries = s.dictionary.clone();
    entries.extend(ov_format::dictionary::builtin_entries());

    // Preview with the user's own edited profiles, not the builtins — otherwise the
    // preview shows what a default install would do, which is the one thing it must
    // not do.
    let p = s
        .profiles
        .iter()
        .find(|p| p.name == profile)
        .cloned()
        .unwrap_or_else(Profile::prose);

    ov_format::Formatter::new(p, &entries)
        .format_traced(&text)
        .trace
        .into_iter()
        .map(|s| (s.stage.to_string(), s.text))
        .collect()
}

/* -- overlay commands ------------------------------------------------------- */

#[tauri::command]
fn overlay_placement(state: tauri::State<'_, AppState>) -> overlay::Placement {
    state.overlay.placement()
}

/// Commit a drag. The frontend reports where the bar ended up; snapping and
/// persistence happen here so the rules live in one place.
#[tauri::command]
fn overlay_move(
    app: AppHandle,
    x: f64,
    y: f64,
    pill_w: f64,
    pill_h: f64,
) -> (f64, f64, overlay::Edge) {
    match overlay::window(&app) {
        Some(win) => app
            .state::<AppState>()
            .overlay
            .move_to(&win, x, y, pill_w, pill_h),
        None => (x, y, overlay::Edge::Bottom),
    }
}

/// Clip the window to the pill. See `Overlay::set_shape`.
///
/// This replaced `overlay_set_box`. The window is a fixed rectangle now, so the
/// frontend has no business knowing where it is or how big it is — it says how
/// big the *pill* is, and the shape follows.
#[tauri::command]
fn overlay_set_shape(
    app: AppHandle,
    pill_w: f64,
    pill_h: f64,
    margin: f64,
    above: Option<bool>,
    view_w: Option<f64>,
    view_h: Option<f64>,
) {
    if let Some(win) = overlay::window(&app) {
        // The layout viewport the pill was actually centred in, not the size this
        // window is nominally supposed to be. They are the same number right up
        // until WebView2 changes its rasterization scale, at which point trusting
        // the constant clips the bar off the screen — see `css_to_physical`.
        //
        // Optional so that a frontend that has not measured itself yet falls back
        // to the nominal size rather than sending a zero.
        let view = (
            view_w.filter(|v| *v > 0.0).unwrap_or(overlay::OVERLAY_W),
            view_h.filter(|v| *v > 0.0).unwrap_or(overlay::OVERLAY_H),
        );
        app.state::<AppState>().overlay.set_shape(
            &win,
            view,
            pill_w,
            pill_h,
            margin,
            above.unwrap_or(false),
        );
    }
}

/// Put a line from the Flow Bar's webview into the app's log.
///
/// The overlay's own instrumentation was written to `console`, which is exactly
/// the wrong place for it: the Flow Bar has no devtools in a release build, and
/// the one surface whose bugs are reported as "it looked wrong for a second" is
/// the one whose evidence nobody can retrieve. Meanwhile the Rust half of the
/// same handshake logs to a file that has been quietly recording every window
/// resize for weeks — and being able to read that is what finally proved the
/// geometry was correct all along and moved the search to the rendering.
///
/// Both halves now land in `openvoice.log`, interleaved and on one clock, so a
/// user reporting this only has to send the file.
#[tauri::command]
fn overlay_log(level: String, msg: String, data: String) {
    match level.as_str() {
        "error" => tracing::error!(%data, "flowbar: {msg}"),
        "warn" => tracing::warn!(%data, "flowbar: {msg}"),
        _ => tracing::debug!(%data, "flowbar: {msg}"),
    }
}

/// Where the bar would snap to if released here. Drives the drag cue.
#[tauri::command]
fn overlay_snap_preview(app: AppHandle, x: f64, y: f64, pill_w: f64, pill_h: f64) -> (f64, f64) {
    match overlay::window(&app) {
        Some(win) => app
            .state::<AppState>()
            .overlay
            .snap_preview(&win, x, y, pill_w, pill_h),
        None => (x, y),
    }
}

/// Discard the dictation in flight.
///
/// The Escape key has always done this — `ov-input`'s hook turns `VK_ESCAPE`
/// into `HotkeyEvent::Cancelled` and the session machine drains to idle — but
/// nothing on screen ever said so, and push-to-talk without a discard path means
/// an accidental trigger *must* be transcribed and injected into someone else's
/// document before it can be undone. This backs the button that finally says it.
#[tauri::command]
fn cancel_session(state: tauri::State<'_, AppState>) {
    if let Some(e) = state.engine.lock().expect("engine").as_ref() {
        e.cancel();
    }
}

/// Start or stop a dictation from the Flow Bar itself. See `Engine::toggle`.
#[tauri::command]
fn toggle_session(state: tauri::State<'_, AppState>) {
    if let Some(e) = state.engine.lock().expect("engine").as_ref() {
        e.toggle();
    }
}

#[tauri::command]
fn overlay_snooze(app: AppHandle, minutes: u64) {
    if let Some(win) = overlay::window(&app) {
        app.state::<AppState>().overlay.snooze(&win, minutes);
    }
}

#[tauri::command]
fn overlay_always_visible(app: AppHandle, on: bool) {
    if let Some(win) = overlay::window(&app) {
        app.state::<AppState>().overlay.set_always_visible(&win, on);
    }
}

/// Cancel a snooze and bring the bar back now.
#[tauri::command]
fn overlay_unsnooze(app: AppHandle) {
    if let Some(win) = overlay::window(&app) {
        app.state::<AppState>().overlay.unsnooze(&win);
    }
}

/// Forget the remembered position and re-place at bottom-centre.
#[tauri::command]
fn overlay_reset_position(app: AppHandle) {
    if let Some(win) = overlay::window(&app) {
        app.state::<AppState>().overlay.reset_position(&win);
    }
}

/// Switch between the compact indicator and the full pill.
#[tauri::command]
fn overlay_set_mini(app: AppHandle, on: bool) {
    app.state::<AppState>().overlay.set_mini(on);
    // Told rather than polled: the bar is the thing that has to re-render, and it
    // may not be the window this command came from -- the Hub's settings toggle
    // reaches the same state.
    if let Some(win) = overlay::window(&app) {
        let _ = win.emit("overlay-mini", on);
    }
}

/// Tell the Windows side whether the Flow Menu is on screen.
///
/// The bar cannot see a click that lands anywhere else — it is
/// `WS_EX_NOACTIVATE`, so it never has focus to lose, and `SetWindowRgn` clips it
/// to the pill, so no click outside the pill is ever delivered to it. The only
/// way to learn about one is a low-level mouse hook, and this is what turns that
/// hook on and off. See `clickaway`.
///
/// Driven by a React effect on the menu's own state, so every route that opens or
/// closes the menu goes through here without each of them having to remember to.
#[tauri::command]
fn overlay_menu_open(app: AppHandle, open: bool) {
    if open {
        clickaway::arm(app);
    } else {
        clickaway::disarm();
    }
}

/// Turn the idle collapse on or off.
///
/// Announced on its own event for the same reason `overlay-mini` is: this can be
/// changed from the bar's own menu or from the Hub, and whichever window did not
/// send it still has to re-render.
#[tauri::command]
fn overlay_set_auto_collapse(app: AppHandle, on: bool) {
    app.state::<AppState>().overlay.set_auto_collapse(on);
    if let Some(win) = overlay::window(&app) {
        let _ = win.emit("overlay-auto-collapse", on);
    }
}

/// Everything the Hub needs to say what the Flow Bar is doing. See
/// `Overlay::state`.
#[tauri::command]
fn overlay_state(app: AppHandle) -> Option<overlay::OverlayState> {
    overlay::window(&app).map(|win| app.state::<AppState>().overlay.state(&win))
}

/// Always log to a file.
///
/// A GUI app launched from a shortcut has nowhere to put stdout — it is detached
/// from any console, so every diagnostic vanishes. That made the first failure
/// impossible to investigate: the app appeared to hang with no output anywhere.
fn init_logging() {
    let dir = history::data_dir();
    let _ = std::fs::create_dir_all(&dir);

    // Crate targets are `ov_app`, `ov_asr`, … with underscores. A directive of
    // `ov=debug` matches nothing, because EnvFilter compares on `::` boundaries
    // rather than as a substring — which silently discarded every log this app
    // emitted about itself.
    // `openvoice` is first because `[[bin]] name = "openvoice"` makes that the
    // binary crate's log target — not `ov_app`, which is only the *package* name.
    // Getting this wrong silently discarded every log the app emitted about
    // itself, while the library crates logged normally, which made it look like
    // the app was not reaching its own startup code at all.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        "openvoice=debug,ov_app=debug,ov_core=debug,ov_asr=debug,\
         ov_audio=debug,ov_input=debug,ov_format=debug,warn"
            .into()
    });

    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("openvoice.log"))
    {
        Ok(file) => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false)
                .with_target(false)
                .init();
        }
        Err(_) => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_target(false)
                .init();
        }
    }
}

fn main() {
    init_logging();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "OpenVoice starting");

    tauri::Builder::default()
        // Registered first, because it is the only thing here that decides whether
        // this process should exist at all.
        //
        // Closing the Hub hides it (see `on_window_event`), so the app is still
        // running when the user launches the shortcut again — and without this,
        // that launch started a *second* complete app: another `WH_KEYBOARD_LL`
        // hook answering the same Right Ctrl, another Flow Bar parked at the same
        // spot, another 1.6 GB of weights, and two processes writing the same
        // history database and `overlay.json`. Holding the hotkey then popped up
        // one bar per process. Plugin setup hooks run before any window is created,
        // so the loser exits having built nothing.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // Launching an app that is already running means "show me the window",
            // which is the same thing the tray does.
            show_hub(app);
        }))
        .plugin(tauri_plugin_opener::init())
        // Registering the plugin does not make a request. Nothing here reaches
        // the network until `update::check` is called, which happens either from
        // a button or from the once-per-launch check the user can turn off.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_ready,
            models::list_models,
            models::download_model,
            models::delete_model,
            models::get_download,
            models::models_on_disk,
            get_history,
            get_totals,
            clear_history,
            get_log_path,
            paste_last,
            open_data_dir,
            overlay_placement,
            overlay_move,
            overlay_set_shape,
            overlay_log,
            overlay_snap_preview,
            overlay_snooze,
            overlay_always_visible,
            overlay_unsnooze,
            overlay_reset_position,
            overlay_set_mini,
            overlay_menu_open,
            overlay_set_auto_collapse,
            overlay_state,
            cancel_session,
            toggle_session,
            show_hub_cmd,
            get_settings,
            save_settings,
            restart_reasons,
            list_microphones,
            retry_engine,
            preview_format,
            check_for_update,
            install_update,
            restart_app
        ])
        .setup(|app| {
            // Built here rather than handed to `manage` in the builder chain: that
            // argument is evaluated before `run` starts, so a second launch would
            // open the history database and apply the retention purge on its way to
            // being killed by the single-instance guard above. Nothing can ask for
            // this state before setup returns — the webviews created a few lines
            // earlier cannot run script until the event loop turns.
            app.manage(AppState::default());

            let handle = app.handle().clone();

            configure_overlay(&handle);
            build_tray(&handle)?;

            // Read from the user's own settings, so "off" is honoured on the very
            // first launch after they turn it off rather than one launch later.
            // Spawned inside, so a slow network cannot delay the window.
            let check_updates = handle
                .state::<AppState>()
                .settings
                .get()
                .config
                .updates
                .check_on_launch;
            update::check_on_launch(&handle, check_updates);

            // Apply the saved placement and visibility policy now, so the bar is on
            // screen before the engine finishes loading. It is the only evidence the
            // user has that anything is happening during those nine seconds.
            if let Some(win) = overlay::window(&handle) {
                handle.state::<AppState>().overlay.apply(&win);
            }

            // Windows takes the bar off the top without telling anyone — see
            // `topmost.rs`. Nothing else in the process would ever notice, which
            // is why the bar appeared to hide itself after a few minutes.
            topmost::spawn_watchdog(handle.clone());

            // Starting the engine loads ~1.6 GB of weights, so it happens off the
            // UI thread. The window paints immediately and reports progress rather
            // than showing a frozen frame for several seconds.
            spawn_engine(handle.clone());

            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the Hub hides it. The app lives in the tray so the hotkey
            // keeps working — quitting is an explicit choice from the tray menu.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "hub" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to start OpenVoice");
}

/// Make the overlay a non-activating tool window.
///
/// This is the single most important line in the shell. Without `WS_EX_NOACTIVATE`
/// the overlay takes focus when shown, the user's caret position is lost, and the
/// dictated text lands in the wrong window — or nowhere. `WS_EX_TOOLWINDOW`
/// additionally keeps it out of Alt-Tab, where a 260px status pill has no business.
#[cfg(windows)]
fn configure_overlay(app: &AppHandle) {
    use std::mem::size_of;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMNCRP_DISABLED, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
        DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW,
    };

    let Some(win) = app.get_webview_window("overlay") else {
        tracing::error!("overlay window missing");
        return;
    };
    let Ok(raw) = win.hwnd() else {
        tracing::error!("overlay has no HWND");
        return;
    };

    let hwnd = HWND(raw.0);
    // SAFETY: `hwnd` is a live window handle owned by this process, and the
    // extended style bits are read back before being modified so no other flag is
    // clobbered.
    unsafe {
        // Suppress Windows 11 DWM 1px accent border, corner lighting, and non-client rendering
        let border_color: u32 = DWMWA_COLOR_NONE;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &border_color as *const _ as *const _,
            size_of::<u32>() as u32,
        );

        let corner_pref: u32 = DWMWCP_DONOTROUND.0 as u32;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner_pref as *const _ as *const _,
            size_of::<u32>() as u32,
        );

        let nc_policy: u32 = DWMNCRP_DISABLED.0 as u32;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_NCRENDERING_POLICY,
            &nc_policy as *const _ as *const _,
            size_of::<u32>() as u32,
        );

        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let wanted = current | (WS_EX_NOACTIVATE.0 as isize) | (WS_EX_TOOLWINDOW.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted);

        // `SetWindowLongPtrW` alone does not finish the job. Win32's contract is
        // that extended-style changes take effect only once `SetWindowPos` is
        // called with `SWP_FRAMECHANGED` to recalculate the frame; until then the
        // window keeps behaving as it did before. The two flags above appeared to
        // work only because the window happens to be shown and moved shortly
        // afterwards, which is luck standing in for a guarantee on the property
        // this whole file calls load-bearing.
        //
        // `HWND_TOPMOST` rides along in the same call: it is the moment the window
        // is first given its z-order, and asking for it here rather than trusting
        // `alwaysOnTop` in tauri.conf.json means one code path owns it. See
        // `topmost.rs`.
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
    tracing::info!(
        "overlay set to non-activating, topmost, and frame-changed with DWM border suppression"
    );
}

#[cfg(not(windows))]
fn configure_overlay(_app: &AppHandle) {}

/// The tray hint, built from the binding the user actually has.
fn tray_tooltip(app: &AppHandle) -> String {
    use ov_core::config::ActivationMode;

    let config = app.state::<AppState>().settings.get().config;
    let key = config.chord.key.label();
    match config.activation {
        ActivationMode::PushToTalk => format!("OpenVoice — hold {key} to dictate"),
        ActivationMode::Toggle => format!("OpenVoice — press {key} to start and stop"),
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open OpenVoice", true, None::<&str>)?;
    let paste = MenuItem::with_id(app, "paste", "Paste last transcript", true, None::<&str>)?;
    let folder = MenuItem::with_id(app, "folder", "Open data folder", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit OpenVoice", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;

    // The Flow Bar's controls used to live exclusively in a menu on the Flow Bar,
    // which is a dead end the moment the bar is not where the user can reach it:
    // snoozed for an hour, switched to dictation-only, dragged somewhere awkward,
    // or -- the case that prompted this -- sunk behind every other window because
    // the shell took its topmost bit away. Every one of those looked identical
    // from the outside, and none of them could be undone without restarting the
    // app. The tray cannot be hidden, so the way back belongs here.
    let bar_show = MenuItem::with_id(app, "bar_show", "Show Flow Bar", true, None::<&str>)?;
    let bar_hide = MenuItem::with_id(app, "bar_hide", "Hide until I dictate", true, None::<&str>)?;
    let bar_reset = MenuItem::with_id(
        app,
        "bar_reset",
        "Reset Flow Bar position",
        true,
        None::<&str>,
    )?;
    let flow_bar = Submenu::with_items(
        app,
        "Flow Bar",
        true,
        &[&bar_show, &bar_hide, &sep, &bar_reset],
    )?;

    let menu = Menu::with_items(
        app,
        &[&open, &paste, &sep, &flow_bar, &sep, &folder, &sep, &quit],
    )?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().expect("bundled icon").clone())
        // Names the key that is actually bound, and the gesture that matches the
        // activation mode. A tray tooltip telling the user to hold a key they
        // rebound is a small lie in the one place they look when confused.
        .tooltip(tray_tooltip(app))
        .menu(&menu)
        // The menu must not open on a plain left click: left click shows the Hub,
        // which is what people reach for.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_hub(app),
            "paste" => {
                if let Some(e) = app
                    .state::<AppState>()
                    .engine
                    .lock()
                    .expect("engine")
                    .as_ref()
                {
                    e.paste_last();
                }
            }
            "folder" => open_data_dir(app.clone()),
            // "Show" clears a snooze *and* re-enables always-visible, because from
            // the tray those are the same wish: the user wants to see the bar, and
            // should not have to know which of the two settings is currently
            // suppressing it.
            "bar_show" => overlay_unsnooze(app.clone()),
            "bar_hide" => overlay_always_visible(app.clone(), false),
            "bar_reset" => overlay_reset_position(app.clone()),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_hub(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

/// Show the Hub, optionally on a named section.
///
/// The Flow Bar's menu offers Microphone, History and Settings as distinct
/// destinations -- Wispr Flow's does the same -- and each of them landing the
/// user on whatever screen the Hub happened to be showing would make the labels
/// decorative. The tab name is a hint, not a command: an unrecognised one leaves
/// the Hub where it was rather than blanking it.
#[tauri::command]
fn show_hub_cmd(app: AppHandle, tab: Option<String>) {
    show_hub(&app);
    if let (Some(tab), Some(win)) = (tab, app.get_webview_window("hub")) {
        let _ = win.emit("hub-navigate", tab);
    }
}

/// Restart the app so a new speech model can be loaded.
///
/// Changing the model means tearing down the sidecar and loading different
/// weights. Telling the user "restart required" and leaving them to find the tray
/// icon is a dead end — the message that states the requirement should also be able
/// to satisfy it.
#[tauri::command]
fn restart_app(app: AppHandle) {
    tracing::info!("restarting to apply a model change");
    app.restart();
}

fn show_hub(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("hub") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
