//! # ov — the OpenVoice command line
//!
//! The composition root, before the GUI exists. It wires every adapter to the pure
//! state machine and runs the real pipeline, which makes it both the first usable
//! build and the integration test harness: `ov transcribe file.wav` exercises the
//! identical code path the GUI will, with no window manager involved.
//!
//! Keeping this binary working is a deliberate constraint. If a feature can only be
//! reached through the GUI, it cannot be tested headlessly, and the pure core stops
//! being pure by degrees.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use clap::{Parser, Subcommand};
use ov_core::config::{Config, SessionLimits};
use ov_core::event::Event;
use ov_core::ports::{
    AppContext, AudioSource, DecodeHint, HotkeyListener, Pcm16k, TextSink, Transcriber,
};
use ov_core::session::{Effect, Input, SessionMachine};
use ov_core::types::{InjectMode, Millis, Outcome};
use ov_format::profile::{self, Profile};
use ov_format::Formatter;

mod history;

#[derive(Parser)]
#[command(
    name = "ov",
    version,
    about = "Local-first voice dictation for developers"
)]
struct Cli {
    /// Model preset: base.en, small.en, or large-v3-turbo.
    #[arg(long, global = true, default_value = "base.en")]
    model: String,

    /// Python interpreter for the ASR sidecar.
    #[arg(long, global = true)]
    python: Option<PathBuf>,

    /// Allow the sidecar to download model weights. Off by default.
    #[arg(long, global = true)]
    allow_download: bool,

    /// Input device name, or part of one. Run `ov devices` to list them.
    /// Defaults to the Windows default input device.
    #[arg(long, global = true)]
    mic: Option<String>,

    /// Seed the model's prompt with the vocabulary. Off by default, and measured
    /// to make output worse: a prompt full of camelCase identifiers teaches the
    /// model to weld ordinary spoken words together. See ov-format's dictionary
    /// module docs for the A/B result.
    #[arg(long, global = true)]
    hint: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Hold the hotkey, speak, release. Text lands at your caret.
    Dictate,
    /// Transcribe a wav file through the full pipeline. No microphone needed.
    Transcribe {
        /// Path to a wav file.
        path: PathBuf,
        /// Profile to format with.
        #[arg(long, default_value = "editor")]
        profile: String,
    },
    /// Run text through the formatter only. Instant, no model.
    Format {
        /// Raw transcript text.
        text: String,
        /// Profile to format with.
        #[arg(long, default_value = "editor")]
        profile: String,
        /// Show the output of every rule.
        #[arg(long)]
        trace: bool,
    },
    /// List input devices.
    Devices,
    /// Check that the environment is ready.
    Doctor,
    /// Print every keystroke the hook sees. Use this when dictation does nothing:
    /// it separates "the hotkey is not reaching us" from every other cause.
    Keytest,
    /// Record from the microphone, report what was captured, save it, and
    /// transcribe it. Isolates the capture path from the hotkey and the state
    /// machine — the one part of the chain a wav-file test cannot reach.
    Mictest {
        /// Seconds to record.
        #[arg(long, default_value_t = 5)]
        seconds: u64,
    },
    /// Type text into whatever window has focus, after a countdown. Tests the
    /// injection path on its own, with no microphone and no model involved.
    Type {
        /// The text to type.
        text: String,
        /// Seconds to wait first, so you can click into the target window.
        #[arg(long, default_value_t = 3)]
        delay: u64,
        /// Force clipboard paste instead of synthesized keystrokes.
        #[arg(long)]
        paste: bool,
    },
}

fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ov=info,warn".into()),
        )
        .with_target(false)
        .compact()
        .init();

    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<(), String> {
    match &cli.command {
        Command::Format {
            text,
            profile,
            trace,
        } => cmd_format(text, profile, *trace),
        Command::Devices => cmd_devices(),
        Command::Doctor => cmd_doctor(cli),
        Command::Transcribe { path, profile } => cmd_transcribe(cli, path, profile),
        Command::Keytest => cmd_keytest(),
        Command::Mictest { seconds } => cmd_mictest(cli, *seconds),
        Command::Type { text, delay, paste } => cmd_type(text, *delay, *paste),
        Command::Dictate => cmd_dictate(cli),
    }
}

/* -- helpers ---------------------------------------------------------------- */

fn pick_profile(name: &str) -> Profile {
    Profile::builtins()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(Profile::editor)
}

/// Locate the directory containing the `sidecar/` package.
///
/// Ordered from most to least explicit, and every candidate is *verified* rather
/// than assumed. An earlier version walked up three levels from the executable and
/// fell back to `"."`, which fails the moment the build directory is not inside the
/// repository — as it is not here, because `CARGO_TARGET_DIR` points at another
/// drive. The fallback then resolved against whatever the working directory
/// happened to be, so the app worked when launched from the repo and died with
/// "The directory name is invalid (os error 267)" when launched from a shortcut.
///
/// The compile-time manifest path is the entry that makes development reliable: it
/// is baked in at build time and does not care where the binary or the working
/// directory end up.
fn find_root() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Some(p) = std::env::var_os("OPENVOICE_ROOT") {
        candidates.push(PathBuf::from(p));
    }

    // Installed layout: sidecar ships beside the executable.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.to_path_buf());
        }
    }

    // Development: crates/ov-cli -> repository root. Known at compile time.
    if let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2) {
        candidates.push(root.to_path_buf());
    }

    // Last resort: anywhere at or above the working directory.
    if let Ok(cwd) = std::env::current_dir() {
        candidates.extend(cwd.ancestors().map(Path::to_path_buf));
    }

    candidates
        .into_iter()
        .find(|p| p.join("sidecar").join("openvoice_asr").is_dir())
}

/// Like [`find_root`], but reports what it tried instead of silently guessing.
fn repo_root() -> Result<PathBuf, String> {
    find_root().ok_or_else(|| {
        format!(
            "could not find the OpenVoice `sidecar` directory.\n  \
             looked beside the executable ({}),\n  \
             at the build-time repo root ({}),\n  \
             and above the working directory ({}).\n  \
             Set OPENVOICE_ROOT to the repository path to fix this.",
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf))
                .unwrap_or_default()
                .display(),
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .unwrap_or(Path::new("?"))
                .display(),
            std::env::current_dir().unwrap_or_default().display(),
        )
    })
}

/// Candidate interpreters, in priority order, for an environment that has
/// faster-whisper installed.
///
/// Shared with `ov-app` in spirit; kept duplicated rather than pulled into a
/// crate because it is fifteen lines and the two binaries are allowed to diverge.
///
/// Every entry is a *convention*, never a specific machine. An absolute path to
/// somebody's development environment compiled into a public binary is both a
/// privacy leak and dead weight for every other user, so if your interpreter is
/// somewhere unusual, say so with `OPENVOICE_PYTHON` (or `--python`) instead of
/// adding a line here:
///
/// ```powershell
/// setx OPENVOICE_PYTHON D:\dev\openvoice-venv\Scripts\python.exe
/// ```
fn python_candidates(root: &Path) -> Vec<PathBuf> {
    let mut c: Vec<PathBuf> = Vec::new();

    // 1. Explicit override. Always wins.
    if let Some(p) = std::env::var_os("OPENVOICE_PYTHON") {
        c.push(PathBuf::from(p));
    }

    // 2. An activated virtualenv in the current shell. Costs nothing to honour
    //    and is what a developer who just ran `activate` expects.
    if let Some(venv) = std::env::var_os("VIRTUAL_ENV") {
        let venv = PathBuf::from(venv);
        c.push(venv.join("Scripts/python.exe"));
        c.push(venv.join("bin/python"));
    }

    // 3. The repo-local environments, in the two places the documented setup
    //    commands create them: `uv venv` at the root, and `uv run` inside
    //    sidecar/.
    for base in [root.join(".venv"), root.join("sidecar/.venv")] {
        c.push(base.join("Scripts/python.exe"));
        c.push(base.join("bin/python"));
    }

    c
}

/// Find the Python interpreter that has faster-whisper installed.
///
/// Every candidate is checked for existence, and failure names what was tried.
/// Falling through to a bare `python` on PATH is deliberately *not* done: it
/// usually exists and usually lacks the dependencies, which produces a confusing
/// import error rather than an honest "no environment found".
fn find_python(root: &Path) -> Result<PathBuf, String> {
    let candidates = python_candidates(root);

    if let Some(found) = candidates.iter().find(|p| p.is_file()) {
        return Ok(found.clone());
    }
    Err(format!(
        "could not find a Python environment with faster-whisper installed.\n  tried:\n{}\n  \
         Set OPENVOICE_PYTHON to the interpreter, or create one with:\n    \
         uv venv && uv pip install -e sidecar nvidia-cublas-cu12 nvidia-cudnn-cu12",
        candidates
            .iter()
            .map(|p| format!("    {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

fn build_transcriber(cli: &Cli) -> Result<ov_asr::SidecarTranscriber, String> {
    let root = repo_root()?;
    let python = match cli.python.clone() {
        Some(p) => p,
        None => find_python(&root)?,
    };
    if !python.is_file() {
        return Err(format!("python not found at {}", python.display()));
    }
    let mut cfg = ov_asr::SidecarConfig::dev(&root, python, &cli.model);
    cfg.allow_download = cli.allow_download;
    ov_asr::SidecarTranscriber::new(cfg).map_err(|e| e.to_string())
}

/* -- format ------------------------------------------------------------------ */

fn cmd_format(text: &str, profile: &str, trace: bool) -> Result<(), String> {
    let f = Formatter::with_builtins(pick_profile(profile));
    if trace {
        for stage in f.format_traced(text).trace {
            println!("{:>12}  {}", stage.stage, stage.text);
        }
    } else {
        println!("{}", f.format(text));
    }
    Ok(())
}

/* -- devices / doctor -------------------------------------------------------- */

fn cmd_devices() -> Result<(), String> {
    let audio = ov_audio::CpalAudioSource::new(None).map_err(|e| e.to_string())?;
    let names = audio.devices().map_err(|e| e.to_string())?;
    if names.is_empty() {
        println!("no input devices found");
        return Ok(());
    }
    println!("Input devices (use with --mic \"<part of the name>\"):\n");
    for name in &names {
        println!("  {name}");
    }
    Ok(())
}

/// Resolve a partial device name to a full one, so `--mic external` works.
fn resolve_mic(want: Option<&String>) -> Result<Option<String>, String> {
    let Some(want) = want else { return Ok(None) };
    let audio = ov_audio::CpalAudioSource::new(None).map_err(|e| e.to_string())?;
    let names = audio.devices().map_err(|e| e.to_string())?;
    let needle = want.to_lowercase();
    match names.iter().find(|n| n.to_lowercase().contains(&needle)) {
        Some(found) => Ok(Some(found.clone())),
        None => Err(format!(
            "no input device matches {want:?}. Available:\n{}",
            names
                .iter()
                .map(|n| format!("  {n}"))
                .collect::<Vec<_>>()
                .join("\n")
        )),
    }
}

fn cmd_doctor(cli: &Cli) -> Result<(), String> {
    // Doctor must report problems, never abort on them: the whole point is to run
    // when something is broken.
    let root = match repo_root() {
        Ok(r) => {
            println!("repo root     ok  {}", r.display());
            println!("sidecar dir   ok  {}", r.join("sidecar").display());
            Some(r)
        }
        Err(e) => {
            println!("repo root     FAILED\n{e}");
            None
        }
    };

    let python = match (&cli.python, &root) {
        (Some(p), _) => Some(p.clone()),
        (None, Some(r)) => match find_python(r) {
            Ok(p) => Some(p),
            Err(e) => {
                println!("python        FAILED\n{e}");
                None
            }
        },
        (None, None) => None,
    };
    if let Some(p) = &python {
        println!("python        ok  {}", p.display());
    }
    println!("model         {}", cli.model);

    match ov_audio::CpalAudioSource::new(None) {
        Ok(a) => {
            let n = a.devices().map(|d| d.len()).unwrap_or(0);
            println!("audio         ok ({n} input devices)");
        }
        Err(e) => println!("audio         FAILED: {e}"),
    }

    match build_transcriber(cli).and_then(|t| {
        t.warm().map_err(|e| e.to_string())?;
        Ok(t.model_id())
    }) {
        Ok(id) => println!("asr           ok ({id})"),
        Err(e) => println!("asr           FAILED: {e}"),
    }

    match ov_input::WinForeground.foreground() {
        Ok(app) => println!("foreground    ok  ({})", app.exe),
        Err(e) => println!("foreground    FAILED: {e}"),
    }
    Ok(())
}

/* -- keytest ----------------------------------------------------------------- */

fn key_name(vk: u32) -> &'static str {
    match vk {
        0xA2 => "LEFT CTRL",
        0xA3 => "RIGHT CTRL  <-- the dictation key",
        0xA0 => "LEFT SHIFT",
        0xA1 => "RIGHT SHIFT",
        0xA4 => "LEFT ALT",
        0xA5 => "RIGHT ALT",
        0x14 => "CAPS LOCK",
        0x1B => "ESCAPE",
        0x20 => "SPACE",
        _ => "",
    }
}

fn cmd_keytest() -> Result<(), String> {
    let rx = ov_input::enable_key_debug();
    let listener = ov_input::WinHotkeyListener::new(Config::default().chord);

    // The port's own events are printed too, so this checks the whole chain:
    // hook -> callback -> channel -> dispatch thread.
    listener
        .start(Arc::new(|event| {
            println!("    >>> HOTKEY EVENT: {event:?}")
        }))
        .map_err(|e| format!("could not install the keyboard hook: {e}"))?;

    println!("Keyboard hook installed. Press some keys — RIGHT CTRL especially.");
    println!("Ctrl+C to quit.\n");

    for (vk, down, injected) in rx {
        let tag = if injected { " (injected)" } else { "" };
        println!(
            "  vk=0x{vk:02X} {:<4}{}{}",
            if down { "down" } else { "up" },
            tag,
            if key_name(vk).is_empty() {
                String::new()
            } else {
                format!("  {}", key_name(vk))
            }
        );
    }
    Ok(())
}

/* -- mictest ----------------------------------------------------------------- */

/// When the binary was built. Printed so a stale running instance is obvious.
///
/// Restarting the app after a rebuild is easy to forget, and the symptom — code
/// changes appearing to have no effect — is indistinguishable from the fix not
/// working. Reading the executable's own mtime needs no build script.
fn build_stamp() -> String {
    std::env::current_exe()
        .and_then(|p| p.metadata())
        .and_then(|m| m.modified())
        .map(|t| {
            let secs = t
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let age = std::time::SystemTime::now()
                .duration_since(t)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            format!("build {secs} ({} min old)", age / 60)
        })
        .unwrap_or_else(|_| "build unknown".into())
}

fn cmd_mictest(cli: &Cli, seconds: u64) -> Result<(), String> {
    let mic = resolve_mic(cli.mic.as_ref())?;
    println!(
        "device: {}",
        mic.clone().unwrap_or_else(|| "<system default>".into())
    );

    let audio = ov_audio::CpalAudioSource::new(mic).map_err(|e| e.to_string())?;
    let peak = Arc::new(Mutex::new(0.0f32));
    let p = peak.clone();
    audio
        .start(Arc::new(move |f: ov_core::ports::LevelFrame| {
            if let Ok(mut m) = p.lock() {
                *m = m.max(f.peak);
            }
        }))
        .map_err(|e| e.to_string())?;

    println!("\nSPEAK NOW - recording for {seconds} seconds\n");
    for remaining in (1..=seconds).rev() {
        print!("  {remaining}... ");
        flush();
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    println!("\n");

    let pcm = audio.stop().map_err(|e| e.to_string())?;

    // Report what actually arrived. A capture path that is subtly wrong -- bad
    // channel count, wrong rate, silent buffer -- shows up here rather than as
    // mysterious garbage from the model three stages later.
    let n = pcm.samples.len();
    let rms = pcm.rms();
    let observed_peak = pcm.samples.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    let clipped = pcm.samples.iter().filter(|s| s.abs() >= 0.999).count();
    let silent = pcm.samples.iter().filter(|s| s.abs() < 1e-5).count();

    println!("captured:");
    println!("  samples      {n} at 16000 Hz mono");
    println!(
        "  duration     {:.2} s (asked for {seconds})",
        n as f64 / 16000.0
    );
    println!("  rms          {rms:.5}");
    println!(
        "  peak         {observed_peak:.5}  (live meter saw {:.5})",
        peak.lock().map(|p| *p).unwrap_or(0.0)
    );
    println!("  clipped      {clipped} samples");
    println!(
        "  near-silent  {silent} of {n} ({:.0}%)",
        100.0 * silent as f64 / n.max(1) as f64
    );

    if (n as f64 / 16000.0 - seconds as f64).abs() > 0.6 {
        println!("  !! DURATION MISMATCH - the capture path is dropping or duplicating audio");
    }
    if rms < 0.002 {
        println!("  !! SIGNAL TOO QUIET - wrong device, muted mic, or capture not working");
    }

    let out = std::env::temp_dir().join("openvoice-mictest.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(&out, spec).map_err(|e| e.to_string())?;
    for s in &pcm.samples {
        w.write_sample((s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16)
            .map_err(|e| e.to_string())?;
    }
    w.finalize().map_err(|e| e.to_string())?;
    println!("\n  saved to {}", out.display());
    println!("  play it back - if it sounds stuttery or wrong, the bug is in capture\n");

    let transcriber = build_transcriber(cli)?;
    transcriber.warm().map_err(|e| e.to_string())?;
    let started = Instant::now();
    let transcript = transcriber
        .transcribe(
            &pcm,
            &DecodeHint {
                vocabulary: vec![],
                language: Some("en".into()),
            },
        )
        .map_err(|e| e.to_string())?;
    let formatter = Formatter::with_builtins(Profile::editor());

    println!("raw    {}", transcript.text);
    println!("final  {}", formatter.format(&transcript.text));
    println!("decode {} ms", started.elapsed().as_millis());
    Ok(())
}

/* -- type -------------------------------------------------------------------- */

fn cmd_type(text: &str, delay: u64, paste: bool) -> Result<(), String> {
    for remaining in (1..=delay).rev() {
        println!("typing into the focused window in {remaining} ...");
        flush();
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    let app = ov_input::WinForeground.foreground().unwrap_or_default();
    println!("target: {} - {}", app.exe, app.title);

    let threshold = Config::default().paste_threshold_chars;
    let mode = if paste {
        InjectMode::ClipboardPaste
    } else {
        ov_input::mode_for(text, threshold)
    };
    println!("mode:   {mode:?}");
    flush();

    match ov_input::WinTextSink::new(threshold).inject(text, mode) {
        Ok(receipt) => {
            println!(
                "ok:     delivered {} chars as {:?}",
                receipt.chars, receipt.mode
            );
            Ok(())
        }
        Err(e) => Err(format!(
            "{e}\n  the text is on your clipboard - press Ctrl+V"
        )),
    }
}

/* -- transcribe a file ------------------------------------------------------- */

fn cmd_transcribe(cli: &Cli, path: &Path, profile: &str) -> Result<(), String> {
    let samples = history::read_wav_16k(path)?;
    let transcriber = build_transcriber(cli)?;
    let formatter = Formatter::with_builtins(pick_profile(profile));

    // Load weights before starting the clock. Without this the reported figure
    // silently includes process spawn and a ~1.6 s model load, which makes every
    // measurement useless for spotting an actual decode regression.
    transcriber.warm().map_err(|e| e.to_string())?;

    let started = Instant::now();
    let hint = DecodeHint {
        vocabulary: if cli.hint {
            formatter.hint_terms().to_vec()
        } else {
            Vec::new()
        },
        language: Some("en".into()),
    };
    let transcript = transcriber
        .transcribe(&Pcm16k { samples }, &hint)
        .map_err(|e| e.to_string())?;
    let decode = started.elapsed();

    let out = formatter.format_traced(&transcript.text);
    println!("raw    {}", transcript.text);
    println!("final  {}", out.text);
    println!("decode {} ms", decode.as_millis());
    Ok(())
}

/* -- the daemon -------------------------------------------------------------- */

struct Runtime {
    audio: ov_audio::CpalAudioSource,
    transcriber: ov_asr::SidecarTranscriber,
    sink: ov_input::WinTextSink,
    apps: ov_input::WinForeground,
    profiles: Vec<Profile>,
    formatters: Vec<(String, Formatter)>,
    captured: Mutex<Option<Pcm16k>>,
    paste_threshold: usize,
    hint: bool,
    start: Instant,
}

impl Runtime {
    fn now(&self) -> Millis {
        Millis(self.start.elapsed().as_millis() as u64)
    }

    fn formatter(&self, profile: &str) -> &Formatter {
        self.formatters
            .iter()
            .find(|(n, _)| n == profile)
            .map(|(_, f)| f)
            .unwrap_or(&self.formatters[0].1)
    }
}

fn cmd_dictate(cli: &Cli) -> Result<(), String> {
    let config = Config::default();
    let transcriber = build_transcriber(cli)?;

    eprintln!("loading {} ...", cli.model);
    transcriber.warm().map_err(|e| e.to_string())?;

    let profiles = Profile::builtins();
    let formatters: Vec<(String, Formatter)> = profiles
        .iter()
        .map(|p| (p.name.clone(), Formatter::with_builtins(p.clone())))
        .collect();

    let mic = resolve_mic(cli.mic.as_ref())?.or_else(|| config.input_device.clone());
    if let Some(name) = &mic {
        println!("microphone: {name}");
    }

    let rt = Arc::new(Runtime {
        audio: ov_audio::CpalAudioSource::new(mic).map_err(|e| e.to_string())?,
        transcriber,
        sink: ov_input::WinTextSink::new(config.paste_threshold_chars),
        apps: ov_input::WinForeground,
        profiles,
        formatters,
        captured: Mutex::new(None),
        paste_threshold: config.paste_threshold_chars,
        hint: cli.hint,
        start: Instant::now(),
    });

    let (tx, rx) = channel::<Input>();

    // Hotkey events become machine inputs. The foreground application is sampled
    // here, on press, because by injection time the user may have switched windows.
    let hotkey = ov_input::WinHotkeyListener::new(config.chord);
    {
        let tx = tx.clone();
        let rt = rt.clone();
        hotkey
            .start(Arc::new(move |event| {
                let at = rt.now();
                let input = match event {
                    ov_core::ports::HotkeyEvent::Pressed => {
                        let app = rt.apps.foreground().unwrap_or_default();
                        let profile = profile::select(&rt.profiles, &app.exe)
                            .map(|p| p.name.clone())
                            .unwrap_or_else(|| "editor".into());
                        Input::HotkeyPressed { at, app, profile }
                    }
                    ov_core::ports::HotkeyEvent::Released => Input::HotkeyReleased { at },
                    ov_core::ports::HotkeyEvent::Cancelled => Input::Cancelled { at },
                };
                let _ = tx.send(input);
            }))
            .map_err(|e| e.to_string())?;
    }

    // Drives the maximum-recording cutoff so a stuck key cannot record forever.
    {
        let tx = tx.clone();
        let rt = rt.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if tx.send(Input::Tick { at: rt.now() }).is_err() {
                break;
            }
        });
    }

    // The build stamp is printed on every start so that "I already fixed that"
    // and "you are running yesterday's binary" can be told apart instantly.
    println!(
        "OpenVoice ready [{}]. Hold RIGHT CTRL and speak. Esc cancels, Ctrl+C quits.",
        build_stamp()
    );

    let mut machine = SessionMachine::new(SessionLimits::default());
    for input in rx {
        for effect in machine.handle(input) {
            execute(&rt, &tx, effect);
        }
    }
    Ok(())
}

/// Progress is printed with `print!` so a session reads as one line. Without an
/// explicit flush none of it appears until the line ends, so the user watches a
/// blank terminal through the exact seconds they are trying to diagnose.
fn flush() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

/// Perform one effect. Anything slow is moved to a worker thread so the state
/// machine loop keeps responding — a blocking decode here would make Esc unusable
/// for the half-second that matters most.
fn execute(rt: &Arc<Runtime>, tx: &Sender<Input>, effect: Effect) {
    match effect {
        Effect::StartCapture { .. } => {
            if let Err(e) = rt.audio.start(Arc::new(|_frame| {})) {
                // Printed, not just logged: a capture that never starts is the
                // difference between "the app is broken" and "pick another input
                // device", and the user cannot see a tracing event.
                println!("\n   MICROPHONE ERROR: {e}");
                println!("   run `ov devices` to see what is available");
                flush();
            }
        }

        Effect::StopCapture { session } => {
            let rt = rt.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let at = rt.now();
                match rt.audio.stop() {
                    Ok(pcm) => {
                        let duration_ms = pcm.duration_ms();
                        let rms = pcm.rms();
                        // Signal level in the open, so a muted or wrong input
                        // device is obvious rather than something to deduce.
                        print!("[{duration_ms} ms, level {rms:.4}] ");
                        flush();
                        *rt.captured.lock().expect("capture mutex poisoned") = Some(pcm);
                        let _ = tx.send(Input::AudioCaptured {
                            session,
                            duration_ms,
                            rms,
                            at,
                        });
                    }
                    Err(e) => {
                        println!("\n   MICROPHONE ERROR: {e}");
                        flush();
                        let _ = tx.send(Input::AudioFailed {
                            session,
                            error: e.to_string(),
                            at,
                        });
                    }
                }
            });
        }

        Effect::AbortCapture { .. } => {
            let _ = rt.audio.abort();
            *rt.captured.lock().expect("capture mutex poisoned") = None;
        }

        Effect::Transcribe { session } => {
            let rt = rt.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let audio = rt.captured.lock().expect("capture mutex poisoned").take();
                let at = rt.now();
                let Some(audio) = audio else {
                    let _ = tx.send(Input::TranscriptionFailed {
                        session,
                        error: "audio buffer was empty".into(),
                        at,
                    });
                    return;
                };
                let hint = DecodeHint {
                    vocabulary: if rt.hint {
                        rt.formatters[0].1.hint_terms().to_vec()
                    } else {
                        Vec::new()
                    },
                    language: Some("en".into()),
                };
                match rt.transcriber.transcribe(&audio, &hint) {
                    Ok(transcript) => {
                        let _ = tx.send(Input::Transcribed {
                            session,
                            transcript,
                            at: rt.now(),
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(Input::TranscriptionFailed {
                            session,
                            error: e.to_string(),
                            at: rt.now(),
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
            // Pure string work, microseconds. Inline is correct here.
            let text = rt.formatter(&profile).format(&raw);
            let _ = tx.send(Input::Formatted {
                session,
                text,
                at: rt.now(),
            });
        }

        Effect::Inject { session, text } => {
            let rt = rt.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mode = ov_input::mode_for(&text, rt.paste_threshold);
                match rt.sink.inject(&text, mode) {
                    Ok(_) => {
                        let _ = tx.send(Input::Injected {
                            session,
                            at: rt.now(),
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(Input::InjectionFailed {
                            session,
                            error: e.to_string(),
                            at: rt.now(),
                        });
                    }
                }
            });
        }

        Effect::Persist { record } => {
            // Every session is recorded, including the failures. "Never lose a
            // word" is only true if the failures are written down too.
            if let Err(e) = history::append(&record) {
                tracing::warn!(error = %e, "could not write history");
            }
            match &record.outcome {
                Outcome::Delivered => {
                    println!("-> {}", record.final_text);
                    println!(
                        "   delivered to {} in {} ms",
                        record.app.exe, record.latency_ms
                    );
                }
                Outcome::ClipboardFallback(text) => {
                    println!("-> {text}");
                    println!("   could not type into {} - press Ctrl+V", record.app.exe);
                }
                Outcome::AsrFailed(e) => println!("   transcription failed: {e}"),
                Outcome::Cancelled => println!("   cancelled"),
                // These two were previously silent, which is the worst possible
                // behaviour when someone is trying to work out why nothing
                // happened. A genuine fat-finger tap deserves no notice; a capture
                // that came back empty because the audio device never started
                // looks identical from here, so print the numbers and let the
                // reader tell them apart.
                Outcome::TooShort => println!(
                    "   ignored: only {} ms of audio (minimum is 300 ms). \
                     If you did hold the key, the microphone is not capturing.",
                    record.audio_ms
                ),
                Outcome::Silent => println!(
                    "   no speech in {} ms of audio - microphone muted or wrong device?",
                    record.audio_ms
                ),
            }
            flush();
        }

        Effect::Emit(event) => match event {
            Event::Listening { profile, .. } => {
                print!("listening [{profile}] ... ");
                flush();
            }
            Event::Transcribing { audio_ms, .. } => {
                print!("{:.1}s captured, decoding ... ", audio_ms as f64 / 1000.0);
                flush();
            }
            Event::Injecting { chars, .. } => {
                print!("typing {chars} chars ... ");
                flush();
            }
            Event::Notice { message, .. } => {
                println!("\n   {message}");
                flush();
            }
            _ => {}
        },
    }
}

/// Injection mode is decided from length; see `ov_input::mode_for`.
const _: fn(&str, usize) -> InjectMode = ov_input::mode_for;
