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

pub mod catalog;
pub mod locate;
pub mod parakeet;
#[cfg(windows)]
mod job;
pub mod store;
mod wav;

/// How to launch the sidecar.
///
/// Two shapes, one struct. In a repository checkout the sidecar is a Python
/// package run by an interpreter from a virtualenv; in an installed copy it is a
/// single frozen executable with no Python anywhere on the machine. The
/// difference is entirely in how the child is spawned, so it lives here as
/// program-and-arguments rather than leaking into [`SidecarTranscriber`].
#[derive(Debug, Clone)]
pub struct SidecarConfig {
    /// The executable to run: a Python interpreter, or the frozen sidecar itself.
    pub program: PathBuf,
    /// Arguments preceding the ones this crate appends. `-m openvoice_asr` when
    /// running from source; empty when the program *is* the sidecar.
    pub leading_args: Vec<String>,
    /// Working directory for the child, if it needs a particular one.
    pub cwd: Option<PathBuf>,
    /// Directory that must contain an `openvoice_asr` package, when running from
    /// source. `None` for a frozen binary, which has no package on disk to check.
    pub package_dir: Option<PathBuf>,
    /// Model preset name, e.g. `base.en` or `large-v3-turbo`.
    pub model: String,
    /// `auto`, `cuda`, or `cpu`.
    pub device: String,
    /// Where transient WAV files are written. They are deleted immediately after
    /// each decode; audio is never retained unless the user opts in.
    pub scratch_dir: PathBuf,
    /// Where model weights are cached.
    ///
    /// `None` leaves the sidecar's own default in place, which is whatever
    /// `HF_HOME` says — right for a developer with a shared cache, wrong for an
    /// installed app, which must keep its weights somewhere it can also delete.
    pub model_dir: Option<PathBuf>,
    /// Directory holding the CUDA libraries, when they were fetched after install.
    ///
    /// The frozen sidecar deliberately ships without cuBLAS and cuDNN — they are
    /// 1.9 GB and worthless on a machine with no NVIDIA GPU. `None` means run on
    /// CPU, which is the correct default and works everywhere.
    pub cuda_dir: Option<PathBuf>,
    /// Where to keep captured audio instead of deleting it, when the user has
    /// asked for that.
    ///
    /// `None` — the default, and what almost every install runs with — means the
    /// scratch WAV is deleted the moment the decode returns, success or failure.
    /// `Some(dir)` backs the "Keep recordings" toggle, which exists for diagnosing
    /// a transcription problem that cannot be reproduced any other way.
    ///
    /// This is the only path in OpenVoice by which audio persists, and it is off
    /// unless deliberately switched on.
    pub retain_audio_dir: Option<PathBuf>,
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
        let sidecar_dir = repo_root.join("sidecar");
        Self {
            program: python,
            leading_args: vec!["-m".into(), "openvoice_asr".into()],
            package_dir: Some(sidecar_dir.clone()),
            cwd: Some(sidecar_dir),
            model: model.to_string(),
            device: "auto".into(),
            scratch_dir: std::env::temp_dir().join("openvoice"),
            model_dir: None,
            cuda_dir: None,
            retain_audio_dir: None,
            allow_download: false,
        }
    }

    /// A configuration for an installed copy, where `exe` is the frozen sidecar.
    ///
    /// The working directory is deliberately left alone. A frozen binary resolves
    /// its own resources relative to itself, and inheriting the parent's directory
    /// keeps the child from pinning a folder open — on Windows that is enough to
    /// make an uninstaller fail.
    #[must_use]
    pub fn bundled(exe: PathBuf, model: &str) -> Self {
        Self {
            program: exe,
            leading_args: Vec::new(),
            package_dir: None,
            cwd: None,
            model: model.to_string(),
            device: "auto".into(),
            scratch_dir: std::env::temp_dir().join("openvoice"),
            model_dir: None,
            cuda_dir: None,
            retain_audio_dir: None,
            allow_download: false,
        }
    }
}

/// How far a long-running operation has got.
///
/// `total` is 0 when the size is not known ahead of time, which the caller should
/// show as an indeterminate bar rather than as 0%.
#[derive(Debug, Clone, Copy)]
pub struct Progress {
    /// Bytes transferred so far.
    pub done: u64,
    /// Bytes expected in total, or 0 if unknown.
    pub total: u64,
}

#[derive(Debug, Deserialize)]
struct Response {
    #[serde(default)]
    ok: bool,
    /// Present only on interim messages. Its absence is what marks a line as the
    /// final response, so it must never be set on one.
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    downloaded: Option<u64>,
    #[serde(default)]
    total: Option<u64>,
    /// Whether `ensure_model` actually transferred anything, as opposed to finding
    /// the weights already cached.
    #[serde(default)]
    fetched: Option<bool>,
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
        // otherwise surfaces as a bare OS error from `spawn` ("The directory name
        // is invalid"), which names neither the path nor the thing that is missing.
        if let Some(dir) = &cfg.package_dir {
            if !dir.join("openvoice_asr").is_dir() {
                return Err(Error::Transcription(format!(
                    "sidecar package not found at {}",
                    dir.join("openvoice_asr").display()
                )));
            }
        }
        // A bare name like `python` is resolved against PATH by the OS, so only a
        // program given with a directory can be checked here.
        if !cfg.program.is_file()
            && cfg
                .program
                .parent()
                .is_some_and(|p| !p.as_os_str().is_empty())
        {
            return Err(Error::Transcription(format!(
                "sidecar program not found at {}",
                cfg.program.display()
            )));
        }
        // Resolve the model here rather than at spawn. A typo in a hand-edited
        // config would otherwise survive construction and surface as a child
        // process exiting during the first decode, which reports as a broken pipe
        // and names neither the model nor the file that set it.
        catalog::resolve(&cfg.model)?;
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

        // The sidecar holds no catalogue of its own: it is told which repository to
        // load and how to quantize it. Resolving here keeps `catalog.rs` the single
        // place a model is described, and means adding one never touches Python.
        let spec = catalog::resolve(&self.cfg.model)?;

        let mut cmd = Command::new(&self.cfg.program);
        cmd.args(&self.cfg.leading_args)
            .arg("--model")
            .arg(&self.cfg.model)
            .arg("--repo")
            .arg(spec.repo)
            .arg("--compute-type")
            .arg(spec.compute_type)
            .arg("--device")
            .arg(&self.cfg.device);
        if let Some(fallback) = spec.fallback_compute {
            cmd.arg("--fallback-compute").arg(fallback);
        }
        if self.cfg.allow_download {
            cmd.arg("--allow-download");
        }
        if let Some(dir) = &self.cfg.cwd {
            cmd.current_dir(dir);
        }
        // Weights go where the app can find *and delete* them. Set both names:
        // `huggingface_hub` reads HF_HOME, and ctranslate2's own loader consults
        // HF_HUB_CACHE. Leaving one unset splits the cache across two directories
        // and silently re-downloads several gigabytes.
        if let Some(dir) = &self.cfg.model_dir {
            cmd.env("HF_HOME", dir).env("HF_HUB_CACHE", dir.join("hub"));
        }
        if let Some(dir) = &self.cfg.cuda_dir {
            cmd.env("OPENVOICE_CUDA_DIR", dir);
        }

        // The sidecar speaks its protocol over stdin and stdout, so it must be a
        // console subsystem binary — and Windows gives every one of those a console
        // window when a GUI process starts it. Without this flag a black window
        // flashes up on every launch and again on every supervisor restart.
        //
        // This suppresses the *window*, not the streams: stdin and stdout stay
        // piped, stderr stays inherited, and the sidecar's diagnostics still reach
        // the log.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // stderr is inherited on purpose: the sidecar's diagnostics land in the
            // app's own log stream, where they are actually read.
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| {
                Error::Transcription(format!(
                    "could not start sidecar ({}): {e}",
                    self.cfg.program.display()
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
        self.request_with_progress(payload, |_| {})
    }

    /// Send a request, forwarding any interim progress to `on_progress`.
    ///
    /// The retry is deliberately not applied to a request that already reported
    /// progress: restarting a download that was half done would throw away the
    /// transferred bytes and begin again, which for a 1.6 GB model is the
    /// difference between a hiccup and starting over.
    fn request_with_progress(
        &self,
        payload: &serde_json::Value,
        mut on_progress: impl FnMut(Progress),
    ) -> Result<Response> {
        let mut progressed = false;
        let first = {
            let mut seen = |p: Progress| {
                progressed = true;
                on_progress(p);
            };
            self.request_once(payload, &mut seen)
        };
        match first {
            Ok(r) => Ok(r),
            Err(e) if progressed => Err(e),
            Err(e) => {
                tracing::warn!(error = %e, "sidecar request failed; restarting once");
                *self.pipe.lock().expect("asr mutex poisoned") = None;
                self.request_once(payload, &mut on_progress)
            }
        }
    }

    fn request_once(
        &self,
        payload: &serde_json::Value,
        on_progress: &mut impl FnMut(Progress),
    ) -> Result<Response> {
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

        // Read until the final response. A long operation may emit any number of
        // interim `event` messages first; anything else on this stream is a
        // protocol violation and is reported rather than skipped, because
        // silently discarding unknown lines is how a desynchronised pipe stays
        // hidden until it returns one request's answer to a different request.
        loop {
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

            if resp.event.is_some() {
                on_progress(Progress {
                    done: resp.downloaded.unwrap_or(0),
                    total: resp.total.unwrap_or(0),
                });
                continue;
            }

            if !resp.ok {
                return Err(Error::Transcription(
                    resp.error
                        .unwrap_or_else(|| "sidecar reported failure".into()),
                ));
            }
            return Ok(resp);
        }
    }

    /// Ensure the configured model's weights are on disk, downloading if needed.
    ///
    /// Cheap and idempotent when they already are, so it is safe to call on every
    /// start. `on_progress` is invoked several times a second during a transfer.
    pub fn ensure_model(&self, on_progress: impl FnMut(Progress)) -> Result<bool> {
        self.ensure_model_named(&self.cfg.model, on_progress)
    }

    /// The cache directory this sidecar is actually using.
    ///
    /// Must be asked of the live configuration rather than assumed. An installed
    /// copy normally keeps weights under its own data directory, but it falls
    /// back to a repository checkout when `OPENVOICE_PYTHON` is set — and then
    /// the cache is wherever `HF_HOME` points instead. Guessing produced a Models
    /// screen that reported an empty cache on a machine holding 1.6 GB.
    #[must_use]
    pub fn hub_dir(&self) -> PathBuf {
        store::hub_dir(self.cfg.model_dir.as_deref())
    }

    /// Fetch *any* catalogued model's weights, not only the loaded one.
    ///
    /// Backs the per-model Download button. Downloading is independent of
    /// loading: it writes to the shared cache and does not disturb the weights
    /// already resident, so the running sidecar can serve it. What it cannot do
    /// is make the app decode with them -- that still needs a restart against the
    /// new model.
    pub fn ensure_model_named(
        &self,
        model: &str,
        mut on_progress: impl FnMut(Progress),
    ) -> Result<bool> {
        let spec = catalog::resolve(model)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let payload = ensure_model_payload(id, model, spec.repo);
        let resp = self.request_with_progress(&payload, &mut on_progress)?;
        Ok(resp.fetched.unwrap_or(false))
    }

    /// Delete the scratch WAV, or move it aside when the user is keeping audio.
    ///
    /// Failing to *keep* a recording falls back to deleting it. That direction is
    /// deliberate and is the only safe one: the alternative on a full disk or a
    /// permissions error is a temp file the user believes was cleaned up.
    fn dispose_of(&self, path: &Path) {
        let Some(dir) = &self.cfg.retain_audio_dir else {
            if let Err(e) = std::fs::remove_file(path) {
                tracing::warn!(path = %path.display(), error = %e, "could not delete scratch audio");
            }
            return;
        };

        let kept = std::fs::create_dir_all(dir).and_then(|()| {
            // Seconds since the epoch, so the folder sorts chronologically and two
            // recordings a second apart cannot collide -- the scratch name carries
            // a per-process counter that restarts with the process.
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "audio".into());
            std::fs::rename(path, dir.join(format!("{stamp}-{name}.wav")))
        });

        match kept {
            Ok(()) => tracing::debug!(dir = %dir.display(), "kept recording"),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    dir = %dir.display(),
                    "could not keep the recording; deleting it instead"
                );
                if let Err(e) = std::fs::remove_file(path) {
                    tracing::warn!(path = %path.display(), error = %e, "could not delete scratch audio");
                }
            }
        }
    }
}

/// Build the `ensure_model` request.
///
/// A field belongs at the **top level**, alongside `id` and `op` — never nested
/// under a literal `"params"` key. `Request.parse` on the Python side treats
/// every field that is not `id` or `op` as a param (`{k: v for k, v in
/// obj.items() if k not in ("id", "op")}`), so a nested `"params"` object is
/// itself stored as one param named `params`, and `req.params.get("model")`
/// silently returns `None` instead of the value. That bug shipped once, unnoticed,
/// only because `ensure_model` has so far always been asked for the same model
/// the sidecar was already launched with, so the handler's `or engine.model_name`
/// fallback happened to land on the right answer anyway. See `transcribe`'s own
/// `"wav": path` for the pattern this follows.
fn ensure_model_payload(id: u64, model: &str, repo: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "op": "ensure_model",
        "model": model,
        "repo": repo,
    })
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

        // Audio does not stay on disk unless the user asked for it. This runs
        // whether the decode succeeded or not, which is the point: a failed
        // transcription must not quietly leave a recording of the user behind.
        self.dispose_of(&path);

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

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact bytes the sidecar writes for an interim progress tick.
    const PROGRESS: &str = r#"{"id":3,"event":"progress","downloaded":524288,"total":1610612736}"#;

    #[test]
    fn a_progress_message_is_not_mistaken_for_a_response() {
        // The reader tells the two apart by `event` alone. If a progress tick ever
        // decoded as a final response, `ensure_model` would return the moment the
        // first byte arrived and `warm` would then load weights that are 0.03%
        // downloaded.
        let r: Response = serde_json::from_str(PROGRESS).expect("decodes");
        assert_eq!(r.event.as_deref(), Some("progress"));
        assert_eq!(r.downloaded, Some(524_288));
        assert_eq!(r.total, Some(1_610_612_736));
        assert!(!r.ok, "a progress tick carries no ok field");
    }

    #[test]
    fn a_final_response_carries_no_event() {
        let r: Response =
            serde_json::from_str(r#"{"id":3,"ok":true,"model":"base.en","fetched":true}"#)
                .expect("decodes");
        assert!(
            r.event.is_none(),
            "presence of `event` is what defers the read"
        );
        assert_eq!(r.fetched, Some(true));
        assert!(r.ok);
    }

    #[test]
    fn fetched_and_downloaded_do_not_collide() {
        // `downloaded` is a byte count on progress and `fetched` is a boolean on the
        // response. They were briefly the same field name, which made every
        // ensure_model reply fail to decode as soon as a download actually happened.
        let cached: Response =
            serde_json::from_str(r#"{"id":1,"ok":true,"model":"base.en","fetched":false}"#)
                .expect("decodes");
        assert_eq!(cached.fetched, Some(false));
        assert_eq!(cached.downloaded, None);
    }

    #[test]
    fn dev_runs_the_package_and_bundled_runs_the_binary() {
        let dev = SidecarConfig::dev(Path::new("C:/repo"), PathBuf::from("python.exe"), "base.en");
        assert_eq!(dev.leading_args, ["-m", "openvoice_asr"]);
        assert!(
            dev.package_dir.is_some(),
            "a source run is validated against the package"
        );

        let bundled = SidecarConfig::bundled(PathBuf::from("C:/app/openvoice-asr.exe"), "base.en");
        assert!(
            bundled.leading_args.is_empty(),
            "the program is the sidecar"
        );
        assert!(
            bundled.package_dir.is_none(),
            "a frozen binary has no package on disk to check for"
        );
        assert!(
            bundled.cwd.is_none(),
            "inheriting a directory would let the child pin a folder the uninstaller must delete"
        );
    }

    #[test]
    fn a_missing_program_is_reported_before_the_first_decode() {
        let cfg = SidecarConfig::bundled(PathBuf::from("C:/nowhere/openvoice-asr.exe"), "base.en");
        // `expect_err` would require SidecarTranscriber: Debug, and deriving Debug on
        // a type holding a live child process and a job handle is not worth it for a
        // test assertion.
        let err = match SidecarTranscriber::new(cfg) {
            Ok(_) => panic!("constructing against a missing binary must fail"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("nowhere"),
            "the error has to name the path it looked at, got: {err}"
        );
    }

    #[test]
    fn ensure_model_puts_model_at_the_top_level_not_under_params() {
        // Pinned against a real regression: the Python side's `Request.parse`
        // stores every field except `id`/`op` as a param, flattened. A payload
        // that nests `{"params": {"model": ...}}` therefore produces a param
        // literally named "params", and `req.params.get("model")` on the Python
        // side silently returns None. It went unnoticed because the handler
        // falls back to the engine's own configured model, which today is
        // always the same value -- correct by coincidence, not by contract.
        let payload = ensure_model_payload(7, "base.en", "Systran/faster-whisper-base.en");
        assert_eq!(payload["id"], 7);
        assert_eq!(payload["op"], "ensure_model");
        assert_eq!(
            payload["model"], "base.en",
            "model must be a top-level field, matching transcribe's own \"wav\": path"
        );
        assert!(
            payload.get("params").is_none(),
            "a nested params object is the exact shape that broke silently before"
        );
    }

    #[test]
    fn ensure_model_carries_the_repo_so_any_model_can_be_fetched() {
        // The Download button fetches a model this sidecar was not started for.
        // Without the repo on the wire the sidecar would fall back to its own,
        // and the button would silently re-download the model already loaded --
        // reporting success for a transfer of the wrong thing.
        let payload = ensure_model_payload(
            1,
            "large-v3-turbo",
            "deepdml/faster-whisper-large-v3-turbo-ct2",
        );
        assert_eq!(payload["repo"], "deepdml/faster-whisper-large-v3-turbo-ct2");
    }

    #[test]
    fn every_catalogued_model_has_a_fetchable_payload() {
        for m in catalog::CATALOG {
            let payload = ensure_model_payload(1, m.id, m.repo);
            assert_eq!(payload["model"], m.id);
            assert_eq!(payload["repo"], m.repo);
        }
    }
}
