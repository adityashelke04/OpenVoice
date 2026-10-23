//! Restartable, memory-only speech worker over bounded binary pipes.
use ov_core::{
    error::{Error, Result},
    ports::{DecodeHint, Pcm16k, Transcriber},
    types::{SessionId, Transcript},
};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

/// Internal executable switch, handled before any UI initialization.
pub const WORKER_FLAG: &str = "--ov-asr-worker";
const MAX_SAMPLES: usize = 16_000 * 60 * 10;
const MAX_STRING: usize = 64 * 1024;
const MAX_TERMS: usize = 512;
const DEADLINE: Duration = Duration::from_secs(60);
type Reply = (u64, std::result::Result<Transcript, String>);
struct Process {
    child: Arc<Mutex<Child>>,
    requests: mpsc::SyncSender<(u64, SessionId, Vec<f32>, DecodeHint)>,
    replies: mpsc::Receiver<io::Result<Reply>>,
}
struct State {
    process: Option<Process>,
    next: u64,
}
/// Serial speech adapter whose native inference runs in a disposable child process.
pub struct WorkerTranscriber {
    exe: PathBuf,
    model: String,
    dir: PathBuf,
    threads: usize,
    deadline: Duration,
    state: Mutex<State>,
    active: Mutex<Option<(SessionId, Arc<Mutex<Child>>)>>,
    cancelled: Mutex<VecDeque<SessionId>>,
}
impl WorkerTranscriber {
    /// Configure the same-executable worker without loading native weights here.
    pub fn new(exe: PathBuf, model: String, dir: PathBuf, threads: usize) -> Self {
        Self {
            exe,
            model,
            dir,
            threads,
            deadline: DEADLINE,
            state: Mutex::new(State {
                process: None,
                next: 0,
            }),
            active: Mutex::new(None),
            cancelled: Mutex::new(VecDeque::new()),
        }
    }
    /// Kill the active child without waiting for the request serialization lock.
    pub fn cancel(&self) {
        if let Some((_, child)) = self.active.lock().unwrap().as_ref() {
            let _ = child.lock().unwrap().kill();
        }
    }
    fn spawn(&self) -> io::Result<Process> {
        let mut command = Command::new(&self.exe);
        command
            .arg(WORKER_FLAG)
            .arg(&self.model)
            .arg(&self.dir)
            .arg(self.threads.to_string());
        self.spawn_command(command)
    }
    fn spawn_command(&self, mut command: Command) -> io::Result<Process> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn()?;
        let mut input = io::BufWriter::new(child.stdin.take().unwrap());
        let mut output = io::BufReader::new(child.stdout.take().unwrap());
        let child = Arc::new(Mutex::new(child));
        let (requests, rx) = mpsc::sync_channel::<(u64, SessionId, Vec<f32>, DecodeHint)>(1);
        let (tx, replies) = mpsc::sync_channel(1);
        let pipe_thread = std::thread::Builder::new()
            .name("ov-asr-pipe".into())
            .spawn(move || {
                while let Ok((id, session, audio, hint)) = rx.recv() {
                    let out = write_request(&mut input, id, session, &audio, &hint)
                        .and_then(|()| read_reply(&mut output));
                    let failed = out.is_err();
                    if tx.send(out).is_err() || failed {
                        break;
                    }
                }
            });
        if let Err(error) = pipe_thread {
            let mut process = child.lock().unwrap();
            let _ = process.kill();
            let _ = process.wait();
            return Err(error);
        }
        Ok(Process {
            child,
            requests,
            replies,
        })
    }
    fn request(
        &self,
        session: SessionId,
        samples: &[f32],
        hint: &DecodeHint,
    ) -> Result<Transcript> {
        if samples.len() > MAX_SAMPLES
            || hint.vocabulary.len() > MAX_TERMS
            || hint.vocabulary.iter().any(|s| s.len() > MAX_STRING)
            || hint.language.as_ref().is_some_and(|s| s.len() > MAX_STRING)
        {
            return Err(Error::Transcription(
                "speech request exceeds memory limit".into(),
            ));
        }
        let mut state = self.state.lock().unwrap();
        if state.process.is_none() {
            state.process = Some(
                self.spawn()
                    .map_err(|e| Error::Transcription(format!("starting speech worker: {e}")))?,
            );
        }
        state.next += 1;
        let id = state.next;
        let process = state.process.as_ref().unwrap();
        {
            let cancelled = self.cancelled.lock().unwrap();
            if cancelled.contains(&session) {
                return Err(Error::Transcription("dictation cancelled".into()));
            }
            *self.active.lock().unwrap() = Some((session, process.child.clone()));
        }
        let result = process
            .requests
            .send((id, session, samples.to_vec(), hint.clone()))
            .map_err(|e| e.to_string())
            .and_then(|()| {
                process
                    .replies
                    .recv_timeout(self.deadline)
                    .map_err(|e| e.to_string())
            })
            .and_then(|r| r.map_err(|e| e.to_string()))
            .and_then(|(received, result)| {
                if received != id {
                    Err("stale speech worker response".into())
                } else {
                    result
                }
            });
        match result {
            Ok(t) => {
                *self.active.lock().unwrap() = None;
                if self.cancelled.lock().unwrap().contains(&session) {
                    Err(Error::Transcription("dictation cancelled".into()))
                } else {
                    Ok(t)
                }
            }
            Err(e) => {
                if let Some(process) = state.process.take() {
                    let mut child = process.child.lock().unwrap();
                    let _ = child.kill();
                    let _ = child.wait();
                }
                *self.active.lock().unwrap() = None;
                Err(Error::Transcription(format!(
                    "speech worker failed ({e}); audio is available to retry"
                )))
            }
        }
    }
}
impl Transcriber for WorkerTranscriber {
    fn warm(&self) -> Result<()> {
        self.request(SessionId(0), &[], &DecodeHint::default())
            .map(|_| ())
    }
    fn transcribe(&self, audio: &Pcm16k, hint: &DecodeHint) -> Result<Transcript> {
        self.request(SessionId(0), &audio.samples, hint)
    }
    fn transcribe_session(
        &self,
        session: SessionId,
        audio: &Pcm16k,
        hint: &DecodeHint,
    ) -> Result<Transcript> {
        self.request(session, &audio.samples, hint)
    }
    fn cancel_session(&self, session: SessionId) {
        let mut cancelled = self.cancelled.lock().unwrap();
        if !cancelled.contains(&session) {
            cancelled.push_back(session);
        }
        if cancelled.len() > 128 {
            cancelled.pop_front();
        }
        if let Some((active, child)) = self.active.lock().unwrap().as_ref() {
            if *active == session {
                let _ = child.lock().unwrap().kill();
            }
        }
    }
    fn model_id(&self) -> String {
        self.model.clone()
    }
}
impl Drop for WorkerTranscriber {
    fn drop(&mut self) {
        self.cancel();
        if let Some(p) = self.state.get_mut().unwrap().process.take() {
            let mut child = p.child.lock().unwrap();
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn number(r: &mut impl Read) -> io::Result<u64> {
    let mut b = [0; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}
fn put(w: &mut impl Write, n: u64) -> io::Result<()> {
    w.write_all(&n.to_le_bytes())
}
fn string(r: &mut impl Read) -> io::Result<String> {
    let n = number(r)?;
    if n > MAX_STRING as u64 {
        return Err(invalid("string limit"));
    }
    let mut bytes = vec![0; n as usize];
    r.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|_| invalid("invalid UTF-8"))
}
fn text(w: &mut impl Write, s: &str) -> io::Result<()> {
    if s.len() > MAX_STRING {
        return Err(invalid("string limit"));
    }
    put(w, s.len() as u64)?;
    w.write_all(s.as_bytes())
}
fn write_request(
    w: &mut impl Write,
    id: u64,
    session: SessionId,
    audio: &[f32],
    hint: &DecodeHint,
) -> io::Result<()> {
    put(w, id)?;
    put(w, session.0)?;
    put(w, audio.len() as u64)?;
    for sample in audio {
        w.write_all(&sample.to_le_bytes())?;
    }
    text(w, hint.language.as_deref().unwrap_or(""))?;
    put(w, hint.vocabulary.len() as u64)?;
    for term in &hint.vocabulary {
        text(w, term)?;
    }
    w.flush()
}
fn read_request(r: &mut impl Read) -> io::Result<(u64, SessionId, Vec<f32>, DecodeHint)> {
    let id = number(r)?;
    let session = SessionId(number(r)?);
    let n = number(r)?;
    if n > MAX_SAMPLES as u64 {
        return Err(invalid("audio limit"));
    }
    let mut bytes = vec![0; n as usize * 4];
    r.read_exact(&mut bytes)?;
    let audio = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&b| f32::from_le_bytes(b))
        .collect();
    let language = string(r)?;
    let n = number(r)?;
    if n > MAX_TERMS as u64 {
        return Err(invalid("vocabulary limit"));
    }
    let vocabulary = (0..n).map(|_| string(r)).collect::<io::Result<_>>()?;
    Ok((
        id,
        session,
        audio,
        DecodeHint {
            vocabulary,
            language: (!language.is_empty()).then_some(language),
        },
    ))
}
fn read_reply(r: &mut impl Read) -> io::Result<Reply> {
    let id = number(r)?;
    let status = number(r)?;
    let value = string(r)?;
    let language = string(r)?;
    Ok((
        id,
        if status == 0 {
            Ok(Transcript {
                text: value,
                language: (!language.is_empty()).then_some(language),
                confidence: None,
            })
        } else {
            Err(value)
        },
    ))
}
/// Run the internal worker if requested. Must be called before initializing Tauri.
pub fn run_from_args() -> Option<Result<()>> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new(WORKER_FLAG)) {
        return None;
    }
    Some((|| {
        let model = args
            .next()
            .and_then(|s| s.into_string().ok())
            .ok_or_else(|| Error::Transcription("missing worker model".into()))?;
        let dir = args
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| Error::Transcription("missing worker model directory".into()))?;
        let threads = args
            .next()
            .and_then(|s| s.into_string().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(4);
        let spec = crate::catalog::find(&model)
            .ok_or_else(|| Error::Transcription("unknown worker model".into()))?;
        let transcriber = crate::sherpa::SherpaTranscriber::with_threads(spec, dir, threads)?;
        serve(
            &transcriber,
            &mut io::BufReader::new(io::stdin().lock()),
            &mut io::BufWriter::new(io::stdout().lock()),
        )
        .map_err(|e| Error::Transcription(format!("speech worker pipe: {e}")))
    })())
}
fn serve(t: &impl Transcriber, r: &mut impl Read, w: &mut impl Write) -> io::Result<()> {
    loop {
        let (id, _session, samples, hint) = match read_request(r) {
            Ok(r) => r,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        };
        let result = if samples.is_empty() {
            Ok(Transcript {
                text: String::new(),
                language: None,
                confidence: None,
            })
        } else {
            t.transcribe(&Pcm16k { samples }, &hint)
        };
        put(w, id)?;
        match result {
            Ok(t) => {
                put(w, 0)?;
                text(w, &t.text)?;
                text(w, t.language.as_deref().unwrap_or(""))?;
            }
            Err(e) => {
                put(w, 1)?;
                text(w, &e.to_string())?;
                text(w, "")?;
            }
        }
        w.flush()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn cancellation_of_an_old_session_does_not_kill_a_newer_request() {
        let worker =
            WorkerTranscriber::new(PathBuf::from("missing"), "fake".into(), PathBuf::new(), 1);
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 30",
        ]);
        let process = worker.spawn_command(command).unwrap();
        *worker.active.lock().unwrap() = Some((SessionId(2), process.child.clone()));
        worker.cancel_session(SessionId(1));
        assert!(process.child.lock().unwrap().try_wait().unwrap().is_none());
        worker.cancel_session(SessionId(2));
        assert!(!process.child.lock().unwrap().wait().unwrap().success());
    }
    #[cfg(windows)]
    #[test]
    fn cancellation_kills_a_blocked_worker_without_waiting_for_the_request_lock() {
        let worker = Arc::new(WorkerTranscriber::new(
            PathBuf::from("missing"),
            "fake".into(),
            PathBuf::new(),
            1,
        ));
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 30",
        ]);
        worker.state.lock().unwrap().process = Some(worker.spawn_command(command).unwrap());
        let decoding = worker.clone();
        let request = std::thread::spawn(move || decoding.warm());
        std::thread::sleep(Duration::from_millis(50));
        let start = std::time::Instant::now();
        worker.cancel();
        assert!(request.join().unwrap().is_err());
        assert!(start.elapsed() < Duration::from_secs(3));
    }
    #[cfg(windows)]
    #[test]
    fn a_stale_reply_is_rejected_and_the_process_is_discarded() {
        let worker =
            WorkerTranscriber::new(PathBuf::from("missing"), "fake".into(), PathBuf::new(), 1);
        let mut response = Vec::new();
        put(&mut response, 999).unwrap();
        put(&mut response, 0).unwrap();
        text(&mut response, "secret stale text").unwrap();
        text(&mut response, "").unwrap();
        let numbers = response
            .iter()
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let script=format!("$o=[Console]::OpenStandardOutput(); $b=[byte[]]@({numbers}); $o.Write($b,0,$b.Length); $o.Flush(); Start-Sleep -Seconds 5");
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        worker.state.lock().unwrap().process = Some(worker.spawn_command(command).unwrap());
        let error = worker.warm().unwrap_err().to_string();
        assert!(error.contains("stale"));
        assert!(!error.contains("secret stale text"));
        assert!(worker.state.lock().unwrap().process.is_none());
    }
    #[cfg(windows)]
    #[test]
    fn hung_or_crashed_child_is_bounded_and_next_worker_can_start() {
        for script in ["Start-Sleep -Seconds 30", "exit 9"] {
            let mut worker =
                WorkerTranscriber::new(PathBuf::from("missing"), "fake".into(), PathBuf::new(), 1);
            worker.deadline = Duration::from_millis(100);
            let mut command = Command::new("powershell.exe");
            command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
            worker.state.lock().unwrap().process = Some(worker.spawn_command(command).unwrap());
            let start = std::time::Instant::now();
            assert!(worker
                .warm()
                .unwrap_err()
                .to_string()
                .contains("audio is available to retry"));
            assert!(start.elapsed() < Duration::from_secs(5));
            assert!(worker.state.lock().unwrap().process.is_none());
            // The next request attempts a fresh process, rather than using a dead pipe.
            assert!(worker
                .warm()
                .unwrap_err()
                .to_string()
                .contains("starting speech worker"));
            let mut response = Vec::new();
            put(&mut response, 2).unwrap();
            put(&mut response, 0).unwrap();
            text(&mut response, "restarted").unwrap();
            text(&mut response, "").unwrap();
            let numbers = response
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let script = format!("$o=[Console]::OpenStandardOutput(); $b=[byte[]]@({numbers}); $o.Write($b,0,$b.Length); $o.Flush(); Start-Sleep -Seconds 5");
            let mut command = Command::new("powershell.exe");
            command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
            worker.deadline = Duration::from_secs(5);
            worker.state.lock().unwrap().process = Some(worker.spawn_command(command).unwrap());
            assert_eq!(
                worker
                    .request(SessionId(0), &[0.1], &DecodeHint::default())
                    .unwrap()
                    .text,
                "restarted"
            );
        }
    }
    #[test]
    fn oversized_binary_input_is_rejected_without_allocating_audio() {
        let mut bytes = Vec::new();
        put(&mut bytes, 1).unwrap();
        put(&mut bytes, 0).unwrap();
        put(&mut bytes, MAX_SAMPLES as u64 + 1).unwrap();
        assert!(read_request(&mut bytes.as_slice())
            .unwrap_err()
            .to_string()
            .contains("audio limit"));
    }
    #[test]
    fn rejects_oversized_audio_before_starting_a_process() {
        let worker =
            WorkerTranscriber::new(PathBuf::from("missing"), "fake".into(), PathBuf::new(), 1);
        let error = worker
            .transcribe(
                &Pcm16k {
                    samples: vec![0.; MAX_SAMPLES + 1],
                },
                &DecodeHint::default(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("limit"));
    }
    #[test]
    fn worker_serves_multiple_requests_and_preserves_hint_and_pcm() {
        struct Echo;
        impl Transcriber for Echo {
            fn warm(&self) -> Result<()> {
                Ok(())
            }
            fn transcribe(&self, a: &Pcm16k, h: &DecodeHint) -> Result<Transcript> {
                Ok(Transcript {
                    text: format!("{}:{}", a.samples[0], h.vocabulary[0]),
                    language: h.language.clone(),
                    confidence: None,
                })
            }
            fn model_id(&self) -> String {
                "echo".into()
            }
        }
        let mut input = Vec::new();
        write_request(&mut input, 1, SessionId(5), &[], &DecodeHint::default()).unwrap();
        write_request(
            &mut input,
            2,
            SessionId(6),
            &[0.25],
            &DecodeHint {
                vocabulary: vec!["proper-name".into()],
                language: Some("en".into()),
            },
        )
        .unwrap();
        let mut output = Vec::new();
        serve(&Echo, &mut input.as_slice(), &mut output).unwrap();
        let mut output = output.as_slice();
        assert_eq!(read_reply(&mut output).unwrap().0, 1);
        let (id, result) = read_reply(&mut output).unwrap();
        assert_eq!(id, 2);
        let text = result.unwrap();
        assert_eq!(text.text, "0.25:proper-name");
        assert_eq!(text.language.as_deref(), Some("en"));
    }
    #[test]
    fn protocol_roundtrips_audio_and_hints() {
        let mut bytes = Vec::new();
        let hint = DecodeHint {
            vocabulary: vec!["a/b secret".into()],
            language: Some("en".into()),
        };
        write_request(&mut bytes, 42, SessionId(7), &[0.1, -0.2], &hint).unwrap();
        let (id, session, audio, decoded) = read_request(&mut bytes.as_slice()).unwrap();
        assert_eq!(id, 42);
        assert_eq!(session, SessionId(7));
        assert_eq!(audio, vec![0.1, -0.2]);
        assert_eq!(decoded, hint);
    }
}
