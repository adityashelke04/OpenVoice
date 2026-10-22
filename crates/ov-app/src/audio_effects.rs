//! Microphone effect boundary shared by the engine and fault-injection tests.
use ov_core::{
    ports::{AudioSource, LevelFrame, Pcm16k, PcmSink},
    session::{Effect, Input, SessionMachine},
    types::{Millis, SessionId},
};
use std::sync::Arc;

pub(crate) fn start(
    audio: &dyn AudioSource,
    session: SessionId,
    levels: Arc<dyn Fn(LevelFrame) + Send + Sync>,
    pcm: PcmSink,
    at: Millis,
) -> Input {
    match audio.start_streaming(levels, pcm) {
        Ok(()) => Input::CaptureStarted { session, at },
        Err(error) => Input::AudioFailed {
            session,
            error: error.to_string(),
            at,
        },
    }
}

pub(crate) fn handle_input(
    machine: &mut SessionMachine,
    audio: &dyn AudioSource,
    input: Input,
) -> (Vec<Effect>, Option<(SessionId, Pcm16k)>) {
    let mut recovery = None;
    if let Input::AudioFailed { session, .. } = &input {
        if machine.capturing_session() == Some(*session) {
            let _ = audio.stop();
            recovery = audio.take_partial().map(|pcm| (*session, pcm));
        }
    }
    (machine.handle(input), recovery)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ov_core::{
        config::SessionLimits,
        error::{Error, Result},
        event::Event,
        types::ForegroundApp,
    };
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Mutex,
    };
    #[derive(Default)]
    struct Mic {
        fail_start: AtomicBool,
        stops: AtomicUsize,
        partial: Mutex<Option<Pcm16k>>,
    }
    impl AudioSource for Mic {
        fn start(&self, _: Arc<dyn Fn(LevelFrame) + Send + Sync>) -> Result<()> {
            if self.fail_start.swap(false, Ordering::SeqCst) {
                Err(Error::Audio("open failed".into()))
            } else {
                Ok(())
            }
        }
        fn stop(&self) -> Result<Pcm16k> {
            self.stops.fetch_add(1, Ordering::SeqCst);
            Err(Error::Audio("device failed".into()))
        }
        fn take_partial(&self) -> Option<Pcm16k> {
            self.partial.lock().unwrap().take()
        }
        fn abort(&self) -> Result<()> {
            Ok(())
        }
        fn devices(&self) -> Result<Vec<String>> {
            Ok(vec![])
        }
    }
    fn press(machine: &mut SessionMachine, at: u64) -> SessionId {
        machine.handle(Input::HotkeyPressed {
            at: Millis(at),
            app: ForegroundApp {
                exe: "editor.exe".into(),
                title: String::new(),
                window: None,
            },
            profile: "editor".into(),
        });
        machine.capturing_session().unwrap()
    }
    fn open(mic: &Mic, id: SessionId) -> Input {
        start(
            mic,
            id,
            Arc::new(|_| {}),
            Arc::new(|_: &[f32]| {}),
            Millis(1),
        )
    }
    #[test]
    fn failed_open_never_announces_listening_and_next_capture_can_start() {
        let mic = Mic::default();
        mic.fail_start.store(true, Ordering::SeqCst);
        let mut machine = SessionMachine::new(SessionLimits::default());
        let first = press(&mut machine, 0);
        let input = open(&mic, first);
        assert!(matches!(input, Input::AudioFailed { .. }));
        let (effects, _) = handle_input(&mut machine, &mic, input);
        assert!(!effects
            .iter()
            .any(|e| matches!(e, Effect::Emit(Event::Listening { .. }))));
        machine.handle(Input::HotkeyReleased { at: Millis(2) });
        let second = press(&mut machine, 3);
        assert_ne!(first, second);
        let (effects, _) = handle_input(&mut machine, &mic, open(&mic, second));
        assert!(effects
            .iter()
            .any(|e| matches!(e,Effect::Emit(Event::Listening {session,..}) if *session==second)));
    }
    #[test]
    fn second_press_while_stop_fails_starts_a_fresh_capture_after_recovery() {
        let mic = Mic::default();
        let mut machine = SessionMachine::new(SessionLimits::default());
        let first = press(&mut machine, 0);
        machine.handle(Input::HotkeyReleased { at: Millis(1000) });
        machine.handle(Input::HotkeyPressed {
            at: Millis(1001),
            app: ForegroundApp {
                exe: "editor.exe".into(),
                title: String::new(),
                window: None,
            },
            profile: "editor".into(),
        });
        assert_eq!(machine.capturing_session(), Some(first));
        *mic.partial.lock().unwrap() = Some(Pcm16k { samples: vec![0.1] });
        let (effects, recovery) = handle_input(
            &mut machine,
            &mic,
            Input::AudioFailed {
                session: first,
                error: "stop failed".into(),
                at: Millis(1002),
            },
        );
        assert!(recovery.is_some());
        let second = effects
            .iter()
            .find_map(|e| match e {
                Effect::StartCapture { session } => Some(*session),
                _ => None,
            })
            .unwrap();
        assert_ne!(first, second);
        assert_eq!(machine.capturing_session(), Some(second));
        let (effects, _) = handle_input(&mut machine, &mic, open(&mic, second));
        assert!(effects
            .iter()
            .any(|e| matches!(e,Effect::Emit(Event::Listening {session,..}) if *session==second)));
    }
    #[test]
    fn stale_failure_does_not_stop_or_steal_partial_from_new_capture() {
        let mic = Mic::default();
        let mut machine = SessionMachine::new(SessionLimits::default());
        let first = press(&mut machine, 0);
        machine.handle(Input::Cancelled { at: Millis(1) });
        machine.handle(Input::HotkeyReleased { at: Millis(2) });
        let second = press(&mut machine, 3);
        *mic.partial.lock().unwrap() = Some(Pcm16k { samples: vec![0.2] });
        let (_, partial) = handle_input(
            &mut machine,
            &mic,
            Input::AudioFailed {
                session: first,
                error: "old device callback".into(),
                at: Millis(4),
            },
        );
        assert_eq!(mic.stops.load(Ordering::SeqCst), 0);
        assert!(partial.is_none());
        assert_eq!(machine.capturing_session(), Some(second));
        assert!(mic.partial.lock().unwrap().is_some());
    }
    #[test]
    fn current_callback_failure_recovers_partial_once_and_rapid_next_press_starts() {
        let mic = Mic::default();
        let mut machine = SessionMachine::new(SessionLimits::default());
        let first = press(&mut machine, 0);
        *mic.partial.lock().unwrap() = Some(Pcm16k {
            samples: vec![0.1, 0.2],
        });
        let (effects, recovery) = handle_input(
            &mut machine,
            &mic,
            Input::AudioFailed {
                session: first,
                error: "unplugged".into(),
                at: Millis(1),
            },
        );
        assert_eq!(
            recovery,
            Some((
                first,
                Pcm16k {
                    samples: vec![0.1, 0.2]
                }
            ))
        );
        assert!(!effects
            .iter()
            .any(|e| matches!(e, Effect::Transcribe { .. })));
        assert!(mic.take_partial().is_none());
        assert_eq!(mic.stops.load(Ordering::SeqCst), 1);
        machine.handle(Input::HotkeyReleased { at: Millis(2) });
        let second = press(&mut machine, 3);
        assert_ne!(first, second);
        assert!(matches!(open(&mic, second), Input::CaptureStarted { .. }));
    }
}
