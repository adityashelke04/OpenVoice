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

use std::sync::{Arc, Mutex};

use ov_core::event::Event;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

mod engine;
mod history;
mod overlay;
mod settings;
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

    fn set_download_progress(&self, progress: Option<engine::DownloadProgress>) {
        *self
            .app
            .state::<AppState>()
            .download
            .lock()
            .expect("download") = progress;
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
    /// First run, fetching model weights. Can last several minutes.
    Downloading(engine::DownloadProgress),
    Ready(engine::Ready),
    Failed {
        error: String,
    },
}

#[tauri::command]
fn get_status(state: tauri::State<'_, AppState>) -> Status {
    // Order matters. A failure outranks everything; a finished engine outranks a
    // stale download record; and only then does an in-flight download outrank the
    // bare "starting", so the UI never shows a progress bar for a run that has
    // already succeeded or failed.
    if let Some(e) = state.error.lock().expect("error").clone() {
        return Status::Failed { error: e };
    }
    if let Some(r) = state.ready.lock().expect("ready").clone() {
        return Status::Ready(r);
    }
    match state.download.lock().expect("download").clone() {
        Some(p) => Status::Downloading(p),
        None => Status::Starting,
    }
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
    /// Set only while first-run weights are being fetched.
    download: Mutex<Option<engine::DownloadProgress>>,
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
            download: Mutex::new(None),
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
    let days = settings.get().config.privacy.history_days;
    if let Err(e) = store.purge_older_than(days) {
        tracing::warn!(error = %e, "retention purge failed");
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
    state: tauri::State<'_, AppState>,
    settings: settings::Settings,
) -> Result<settings::Settings, String> {
    let restart_needed = {
        let current = state.settings.get();
        current.model != settings.model
    };
    let saved = state.settings.update(|s| *s = settings)?;

    // Push the edited dictionary into the running engine so corrections take effect
    // on the very next utterance. Without this the Dictionary screen would quietly
    // require a restart, and the interface says otherwise.
    if let Some(e) = state.engine.lock().expect("engine").as_ref() {
        e.reload_rules(&saved);
        e.reload_language(&saved);
    }

    if restart_needed {
        tracing::info!(model = %saved.model, "model changed; restart required");
    }
    Ok(saved)
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

/// The models this build can load.
///
/// Served from `ov_asr::catalog`, which is the only place a model is described.
/// The Models screen used to carry its own copy of this list — ids, sizes and all
/// — so a model added to the sidecar simply never appeared, and a size corrected
/// in one file silently disagreed with the other.
///
/// Deliberately does not require a running engine: the catalogue is static data,
/// and a settings screen that cannot render until the speech engine is warm would
/// be unusable during exactly the first-run download it needs to explain.
#[tauri::command]
fn list_models() -> Vec<ov_asr::catalog::ModelSpec> {
    ov_asr::catalog::CATALOG.to_vec()
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
fn overlay_move(app: AppHandle, x: f64, y: f64) {
    if let Some(win) = overlay::window(&app) {
        app.state::<AppState>().overlay.move_to(&win, x, y);
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
            get_history,
            get_totals,
            clear_history,
            get_log_path,
            paste_last,
            open_data_dir,
            overlay_placement,
            overlay_move,
            overlay_snooze,
            overlay_always_visible,
            show_hub_cmd,
            get_settings,
            save_settings,
            list_microphones,
            list_models,
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

            // Starting the engine loads ~1.6 GB of weights, so it happens off the
            // UI thread. The window paints immediately and reports progress rather
            // than showing a frozen frame for several seconds.
            let bg = handle.clone();
            std::thread::spawn(move || {
                let shell = Arc::new(TauriShell { app: bg.clone() });

                let mut settings = bg.state::<AppState>().settings.get();
                // The environment variable still wins, so a stuck configuration can
                // always be overridden from a shortcut without editing a file.
                if let Ok(m) = std::env::var("OPENVOICE_MODEL") {
                    settings.model = m;
                }

                let store = bg.state::<AppState>().store.clone();
                // Where the frozen speech engine lives in an installed copy. `Err`
                // simply means this is not one, and the engine falls back to a
                // repository checkout.
                let resources = bg.path().resource_dir().ok();
                match engine::start(shell, &settings, store, resources.as_deref()) {
                    Ok((engine, ready)) => {
                        let state = bg.state::<AppState>();
                        *state.engine.lock().expect("engine") = Some(engine);
                        *state.ready.lock().expect("ready") = Some(ready.clone());
                        let _ = bg.emit("ov://ready", ready);
                        tracing::info!("engine ready");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "engine failed to start");
                        // Recorded in state as well as emitted: the event can be
                        // missed, the state cannot.
                        *bg.state::<AppState>().error.lock().expect("error") = Some(e.clone());
                        let _ = bg.emit("ov://error", e);
                    }
                }
            });

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
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
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
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let wanted = current | (WS_EX_NOACTIVATE.0 as isize) | (WS_EX_TOOLWINDOW.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted);
    }
    tracing::info!("overlay set to non-activating");
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

    let menu = Menu::with_items(app, &[&open, &paste, &sep, &folder, &sep, &quit])?;

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

#[tauri::command]
fn show_hub_cmd(app: AppHandle) {
    show_hub(&app);
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
