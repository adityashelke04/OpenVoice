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

use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use clap::{Parser, Subcommand};
use ov_core::config::{Config, SessionLimits};
use ov_core::event::Event;
use ov_core::ports::{AppContext, AudioSource, DecodeHint, HotkeyListener, Pcm16k, TextSink, Transcriber};
use ov_core::session::{Effect, Input, SessionMachine};
use ov_core::types::{InjectMode, Millis, Outcome};
use ov_format::profile::{self, Profile};
use ov_format::Formatter;

mod history;

#[derive(Parser)]
#[command(name = "ov", version, about = "Local-first voice dictation for developers")]
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
        Command::Format { text, profile, trace } => cmd_format(text, profile, *trace),
        Command::Devices => cmd_devices(),
        Command::Doctor => cmd_doctor(cli),
        Command::Transcribe { path, profile } => cmd_transcribe(cli, path, profile),
        Command::Keytest => cmd_keytest(),
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

fn repo_root() -> PathBuf {
    // The binary lives in target/<profile>/, so the repository is three levels up
    // during development. Falls back to the working directory when installed.
    std::env::current_exe()
        .ok()
        .and_then(|p| p.ancestors().nth(3).map(PathBuf::from))
        .filter(|p| p.join("sidecar").is_dir())
        .or_else(|| {
            let cwd = std::env::current_dir().ok()?;
            cwd.ancestors().find(|p| p.join("sidecar").is_dir()).map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from("."))
}

fn default_python(root: &std::path::Path) -> PathBuf {
    for candidate in [
        PathBuf::from("D:/dev/openvoice-venv/Scripts/python.exe"),
        root.join(".venv/Scripts/python.exe"),
        root.join(".venv/bin/python"),
    ] {
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from("python")
}

fn build_transcriber(cli: &Cli) -> Result<ov_asr::SidecarTranscriber, String> {
    let root = repo_root();
    let python = cli.python.clone().unwrap_or_else(|| default_python(&root));
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
    for name in audio.devices().map_err(|e| e.to_string())? {
        println!("{name}");
    }
    Ok(())
}

fn cmd_doctor(cli: &Cli) -> Result<(), String> {
    let root = repo_root();
    let python = cli.python.clone().unwrap_or_else(|| default_python(&root));
    println!("repo root     {}", root.display());
    println!("sidecar dir   {}", root.join("sidecar").display());
    println!("python        {}", python.display());
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
        Ok(app) => println!("foreground    ok ({})", app.exe),
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
        .start(Arc::new(|event| println!("    >>> HOTKEY EVENT: {event:?}")))
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

/* -- transcribe a file ------------------------------------------------------- */

fn cmd_transcribe(cli: &Cli, path: &PathBuf, profile: &str) -> Result<(), String> {
    let samples = history::read_wav_16k(path)?;
    let transcriber = build_transcriber(cli)?;
    let formatter = Formatter::with_builtins(pick_profile(profile));

    // Load weights before starting the clock. Without this the reported figure
    // silently includes process spawn and a ~1.6 s model load, which makes every
    // measurement useless for spotting an actual decode regression.
    transcriber.warm().map_err(|e| e.to_string())?;

    let started = Instant::now();
    let hint = DecodeHint {
        vocabulary: if cli.hint { formatter.hint_terms().to_vec() } else { Vec::new() },
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

    let rt = Arc::new(Runtime {
        audio: ov_audio::CpalAudioSource::new(config.input_device.clone())
            .map_err(|e| e.to_string())?,
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

    println!("OpenVoice ready. Hold RIGHT CTRL and speak. Esc cancels, Ctrl+C quits.");

    let mut machine = SessionMachine::new(SessionLimits::default());
    for input in rx {
        for effect in machine.handle(input) {
            execute(&rt, &tx, effect);
        }
    }
    Ok(())
}

/// Perform one effect. Anything slow is moved to a worker thread so the state
/// machine loop keeps responding — a blocking decode here would make Esc unusable
/// for the half-second that matters most.
fn execute(rt: &Arc<Runtime>, tx: &Sender<Input>, effect: Effect) {
    match effect {
        Effect::StartCapture { .. } => {
            if let Err(e) = rt.audio.start(Arc::new(|_frame| {})) {
                tracing::error!(error = %e, "could not start capture");
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
                        *rt.captured.lock().expect("capture mutex poisoned") = Some(pcm);
                        let _ = tx.send(Input::AudioCaptured { session, duration_ms, rms, at });
                    }
                    Err(e) => {
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
                        let _ = tx.send(Input::Transcribed { session, transcript, at: rt.now() });
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

        Effect::Format { session, raw, profile } => {
            // Pure string work, microseconds. Inline is correct here.
            let text = rt.formatter(&profile).format(&raw);
            let _ = tx.send(Input::Formatted { session, text, at: rt.now() });
        }

        Effect::Inject { session, text } => {
            let rt = rt.clone();
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mode = ov_input::mode_for(&text, rt.paste_threshold);
                match rt.sink.inject(&text, mode) {
                    Ok(_) => {
                        let _ = tx.send(Input::Injected { session, at: rt.now() });
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
                    println!("  {} ({} ms)", record.final_text, record.latency_ms);
                }
                Outcome::ClipboardFallback(text) => {
                    println!("  [clipboard] {text}");
                    println!("  press Ctrl+V to paste");
                }
                Outcome::AsrFailed(e) => eprintln!("  transcription failed: {e}"),
                Outcome::TooShort | Outcome::Cancelled | Outcome::Silent => {}
            }
        }

        Effect::Emit(event) => match event {
            Event::Listening { .. } => print!("listening ... "),
            Event::Transcribing { audio_ms, .. } => {
                print!("{:.1}s captured, decoding ... ", audio_ms as f64 / 1000.0);
                use std::io::Write;
                let _ = std::io::stdout().flush();
            }
            Event::Notice { message, .. } => println!("  {message}"),
            _ => {}
        },
    }
}

/// Injection mode is decided from length; see `ov_input::mode_for`.
const _: fn(&str, usize) -> InjectMode = ov_input::mode_for;
