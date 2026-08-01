//! # ov-asr — speech recognition
//!
//! Implements [`ov_core::ports::Transcriber`] by supervising the Python sidecar
//! described in ADR 0003, speaking newline-delimited JSON over its stdin and stdout.
//!
//! ## Why a child process
//!
//! faster-whisper gets CUDA working from a pip wheel with no compiler, which was
//! worth more on day one than the self-contained binary whisper.cpp would give. The
//! cost — bundling a Python runtime at distribution time — is paid later, and the
//! `Transcriber` trait keeps the swap cheap.
//!
//! Process isolation turns out to be a real benefit rather than just a consequence:
//! a CUDA fault or an out-of-memory kill takes down the sidecar, not the app, and
//! the supervisor restarts it. An in-process backend would take the whole app with
//! it and lose the user's audio.
//!
//! ## Lifetime
//!
//! The sidecar exits when its stdin closes, which happens automatically when this
//! struct is dropped or the parent process dies. That is what stops an orphaned
//! Python process from sitting on 1.6 GB of VRAM.

// `unsafe` is confined to `job.rs`, which uses Win32 job objects to guarantee the
// sidecar cannot outlive this process and strand GPU memory. Everything else in
// this crate is safe Rust.
#![warn(missing_docs, clippy::all)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use ov_core::error::{Error, Result};
use ov_core::ports::{DecodeHint, Pcm16k, Transcriber};
use ov_core::types::Transcript;
use serde::Deserialize;

#[cfg(windows)]
mod job;
mod wav;

/// How to launch the sidecar.
#[derive(Debug, Clone)]
pub struct SidecarConfig {
    /// Python interpreter, ideally the one in the project's virtual environment.
    pub python: PathBuf,
    /// Directory containing the `openvoice_asr` package.
    pub sidecar_dir: PathBuf,
    /// Model preset name, e.g. `base.en` or `large-v3-turbo`.
    pub model: String,
    /// `auto`, `cuda`, or `cpu`.
    pub device: String,
    /// Where transient WAV files are written. They are deleted immediately after
    /// each decode; audio is never retained unless the user opts in.
    pub scratch_dir: PathBuf,
    /// Permit the sidecar to fetch weights from Hugging Face.
    ///
    /// Off by default. The sidecar is inference-only, and leaving the network
    /// reachable costs ~171 s per load even for an already-cached model, because
    /// `huggingface_hub` blocks revalidating it.
    pub allow_download: bool,
}

impl SidecarConfig {
    /// A configuration rooted at a repository checkout, for development.
    #[must_use]
    pub fn dev(repo_root: &Path, python: PathBuf, model: &str) -> Self {
        Self {
            python,
            sidecar_dir: repo_root.join("sidecar"),
            model: model.to_string(),
            device: "auto".into(),
            scratch_dir: std::env::temp_dir().join("openvoice"),
            allow_download: false,
        }
    }
}

#[derive(Debug, Deserialize)]
struct Response {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    confidence: Option<f32>,
    #[serde(default)]
    decode_ms: Option<u64>,
    #[serde(default)]
    model_id: Option<String>,
    #[serde(default)]
    device: Option<String>,
}

struct Pipe {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

/// `Transcriber` backed by the faster-whisper sidecar.
pub struct SidecarTranscriber {
    cfg: SidecarConfig,
    pipe: Mutex<Option<Pipe>>,
    next_id: AtomicU64,
    model_id: Mutex<String>,
    /// Kernel-enforced guarantee that the sidecar cannot outlive this process.
    /// Closing stdin covers a graceful exit; this covers a kill or a crash.
    #[cfg(windows)]
    job: Option<job::KillOnDrop>,
}

impl SidecarTranscriber {
    /// Create a supervisor. The process is not spawned until first use or
    /// [`Transcriber::warm`].
    pub fn new(cfg: SidecarConfig) -> Result<Self> {
        // Validate at construction, not at first decode. A missing sidecar
        // directory otherwise surfaces as a bare OS error from `spawn` ("The
        // directory name is invalid"), which names neither the directory nor the
        // thing that is missing.
        if !cfg.sidecar_dir.join("openvoice_asr").is_dir() {
            return Err(Error::Transcription(format!(
                "sidecar package not found at {}",
                cfg.sidecar_dir.join("openvoice_asr").display()
            )));
        }
        if !cfg.python.is_file()
            && cfg
                .python
                .parent()
                .is_some_and(|p| !p.as_os_str().is_empty())
        {
            return Err(Error::Transcription(format!(
                "python interpreter not found at {}",
                cfg.python.display()
            )));
        }
        std::fs::create_dir_all(&cfg.scratch_dir)
            .map_err(|e| Error::Transcription(format!("creating scratch dir: {e}")))?;
        let model_id = Mutex::new(format!("faster-whisper/{}", cfg.model));

        #[cfg(windows)]
        let job = {
            let j = job::KillOnDrop::new();
            if j.is_none() {
                tracing::warn!(
                    "could not create a job object; a hard kill of this process may \
                     strand the sidecar holding GPU memory"
                );
            }
            j
        };

        Ok(Self {
            cfg,
            pipe: Mutex::new(None),
            next_id: AtomicU64::new(1),
            model_id,
            #[cfg(windows)]
            job,
        })
    }

    fn spawn(&self) -> Result<Pipe> {
        tracing::info!(model = %self.cfg.model, device = %self.cfg.device, "starting ASR sidecar");

        let mut cmd = Command::new(&self.cfg.python);
        cmd.arg("-m")
            .arg("openvoice_asr")
            .arg("--model")
            .arg(&self.cfg.model)
            .arg("--device")
            .arg(&self.cfg.device);
        if self.cfg.allow_download {
            cmd.arg("--allow-download");
        }

        let mut child = cmd
            .current_dir(&self.cfg.sidecar_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // stderr is inherited on purpose: the sidecar's diagnostics land in the
            // app's own log stream, where they are actually read.
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                Error::Transcription(format!(
                    "could not start sidecar ({}): {e}",
                    self.cfg.python.display()
                ))
            })?;

        // Adopt before anything else can go wrong, so the child is covered for its
        // entire life rather than from the first successful request onwards.
        #[cfg(windows)]
        if let Some(job) = &self.job {
            if !job.adopt(&child) {
                tracing::warn!("could not assign the sidecar to the job object");
            }
        }

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Transcription("sidecar stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Transcription("sidecar stdout unavailable".into()))?;

        Ok(Pipe {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    /// Send one request and read its response, restarting the sidecar once if the
    /// pipe is broken.
    ///
    /// The single retry is deliberate. A dead sidecar is worth one restart, because
    /// the audio in hand is unrecoverable if we give up. Retrying repeatedly would
    /// turn a reproducible failure into a loop that reloads 1.6 GB of weights each
    /// time, which is worse than reporting the error.
    fn request(&self, payload: &serde_json::Value) -> Result<Response> {
        match self.request_once(payload) {
            Ok(r) => Ok(r),
            Err(first) => {
                tracing::warn!(error = %first, "sidecar request failed; restarting once");
                *self.pipe.lock().expect("asr mutex poisoned") = None;
                self.request_once(payload)
            }
        }
    }

    fn request_once(&self, payload: &serde_json::Value) -> Result<Response> {
        let mut guard = self.pipe.lock().expect("asr mutex poisoned");
        if guard.is_none() {
            *guard = Some(self.spawn()?);
        }
        let pipe = guard.as_mut().expect("just populated");

        let line = serde_json::to_string(payload)
            .map_err(|e| Error::Transcription(format!("encoding request: {e}")))?;
        pipe.stdin
            .write_all(line.as_bytes())
            .and_then(|()| pipe.stdin.write_all(b"\n"))
            .and_then(|()| pipe.stdin.flush())
            .map_err(|e| Error::Transcription(format!("writing to sidecar: {e}")))?;

        let mut buf = String::new();
        let read = pipe
            .stdout
            .read_line(&mut buf)
            .map_err(|e| Error::Transcription(format!("reading from sidecar: {e}")))?;
        if read == 0 {
            return Err(Error::Transcription("sidecar closed its output".into()));
        }

        let resp: Response = serde_json::from_str(buf.trim())
            .map_err(|e| Error::Transcription(format!("decoding response {buf:?}: {e}")))?;

        if !resp.ok {
            return Err(Error::Transcription(
                resp.error
                    .unwrap_or_else(|| "sidecar reported failure".into()),
            ));
        }
        Ok(resp)
    }
}

impl Drop for SidecarTranscriber {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.pipe.lock() {
            if let Some(mut pipe) = guard.take() {
                // Closing stdin is the sidecar's shutdown signal; the kill is only
                // a backstop for a process wedged inside CUDA.
                drop(pipe.stdin);
                std::thread::sleep(std::time::Duration::from_millis(150));
                let _ = pipe.child.kill();
                let _ = pipe.child.wait();
            }
        }
    }
}

impl Transcriber for SidecarTranscriber {
    fn warm(&self) -> Result<()> {
        let resp = self.request(&serde_json::json!({ "id": 0, "op": "warm" }))?;
        if let Some(id) = resp.model_id {
            *self.model_id.lock().expect("model id mutex poisoned") = id;
        }
        tracing::info!(device = ?resp.device, "ASR model loaded");
        Ok(())
    }

    fn transcribe(&self, audio: &Pcm16k, hint: &DecodeHint) -> Result<Transcript> {
        if audio.samples.is_empty() {
            return Err(Error::Transcription("no audio to transcribe".into()));
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let path = self.cfg.scratch_dir.join(format!("ov-{id}.wav"));
        wav::write_16k_mono(&path, &audio.samples)
            .map_err(|e| Error::Transcription(format!("writing scratch wav: {e}")))?;

        let payload = serde_json::json!({
            "id": id,
            "op": "transcribe",
            "wav": path.to_string_lossy(),
            "vocabulary": hint.vocabulary,
            "language": hint.language,
        });

        let result = self.request(&payload);

        // Audio is never left on disk. This runs whether the decode succeeded or
        // not, which is the point: a failed transcription must not quietly leave a
        // recording of the user behind.
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!(path = %path.display(), error = %e, "could not delete scratch audio");
        }

        let resp = result?;
        if let Some(id) = resp.model_id {
            *self.model_id.lock().expect("model id mutex poisoned") = id;
        }
        if let Some(ms) = resp.decode_ms {
            tracing::debug!(decode_ms = ms, "decoded");
        }

        Ok(Transcript {
            text: resp.text.unwrap_or_default(),
            language: resp.language,
            confidence: resp.confidence,
        })
    }

    fn model_id(&self) -> String {
        self.model_id
            .lock()
            .expect("model id mutex poisoned")
            .clone()
    }
}
