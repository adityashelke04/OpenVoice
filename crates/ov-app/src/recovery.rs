//! One expiring, bounded in-memory audio recovery slot.

pub struct Retained<T> {
    pub generation: u64,
    pub session: u64,
    pub value: T,
    pub samples: usize,
    pub partial: bool,
    expires: u64,
}

pub struct Recovery<T> {
    entry: Option<Retained<T>>,
    generation: u64,
}

#[derive(Default)]
pub struct LatestText {
    session: u64,
    pub text: String,
    pub partial: bool,
}

impl LatestText {
    pub fn remember(&mut self, session: u64, text: String) {
        if session < self.session || text.is_empty() {
            return;
        }
        self.session = session;
        self.text = text;
        self.partial = false;
    }

    pub fn remember_partial(&mut self, session: u64, text: String) {
        if session < self.session || text.is_empty() {
            return;
        }
        self.remember(session, text);
        self.partial = true;
    }
}

impl<T> Default for Recovery<T> {
    fn default() -> Self {
        Self {
            entry: None,
            generation: 0,
        }
    }
}

impl<T> Recovery<T> {
    pub fn keep(&mut self, session: u64, value: T, samples: usize, partial: bool, now: u64) {
        if samples == 0 || samples > 9_600_000 {
            return;
        }
        self.generation += 1;
        self.entry = Some(Retained {
            generation: self.generation,
            session,
            value,
            samples,
            partial,
            expires: now + 600_000,
        });
    }
    pub fn get(&mut self, now: u64) -> Option<&Retained<T>> {
        if self.entry.as_ref().is_some_and(|v| now >= v.expires) {
            self.clear();
        }
        self.entry.as_ref()
    }
    pub fn clear(&mut self) {
        self.generation += 1;
        self.entry = None;
    }

    pub fn clear_if_current(&mut self, session: u64, generation: u64, now: u64) -> bool {
        if self.is_current(session, generation, now) {
            self.clear();
            true
        } else {
            false
        }
    }

    pub fn is_current(&mut self, session: u64, generation: u64, now: u64) -> bool {
        self.get(now)
            .is_some_and(|r| r.session == session && r.generation == generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_retry_cannot_clear_new_or_discarded_recovery() {
        let mut recovery = Recovery::default();
        recovery.keep(1, "old", 16000, false, 0);
        let old = recovery.get(1).unwrap().generation;
        recovery.keep(2, "new", 16000, false, 2);
        assert!(!recovery.clear_if_current(1, old, 3));
        assert_eq!(recovery.get(3).unwrap().value, "new");
        let generation = recovery.get(3).unwrap().generation;
        recovery.clear();
        assert!(!recovery.clear_if_current(2, generation, 4));
    }

    #[test]
    fn partial_metadata_cannot_label_a_newer_complete_transcript() {
        let mut latest = LatestText::default();
        latest.remember(2, "complete".into());
        latest.remember_partial(1, "incomplete old".into());
        assert!(!latest.partial);
        latest.remember_partial(3, "current partial".into());
        assert!(latest.partial);
    }

    #[test]
    fn late_completion_cannot_replace_a_newer_recovery_transcript() {
        let mut latest = LatestText::default();
        latest.remember(2, "new complete text".into());
        latest.remember(1, "old cancelled text".into());
        assert_eq!(latest.text, "new complete text");
    }

    #[test]
    fn audio_expires_without_a_successful_retry() {
        let mut r = Recovery::default();
        r.keep(1, "audio", 16_000, true, 10);
        assert_eq!(r.get(600_009).unwrap().session, 1);
        assert!(r.get(600_010).is_none());
    }

    #[test]
    fn retention_is_bounded_to_one_valid_ten_minute_utterance() {
        let mut r = Recovery::default();
        r.keep(1, "audio", 16_000, false, 0);
        r.keep(2, "too large", 9_600_001, false, 1);
        assert_eq!(r.get(2).unwrap().value, "audio");
        r.keep(3, "new", 32_000, true, 2);
        assert_eq!(r.get(3).unwrap().session, 3);
        r.clear();
        assert!(r.get(3).is_none());
    }
}
