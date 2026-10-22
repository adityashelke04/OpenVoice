//! A clipboard lease survives consecutive dictations, but never a third-party copy.

#[derive(Default)]
pub(crate) struct Lease<T> {
    pub snapshot: Option<T>,
    pub sequence: u32,
    pub generation: u64,
}

impl<T> Lease<T> {
    pub fn take_for_restore(&mut self, generation: u64, current: u32) -> Option<T> {
        if generation != self.generation {
            return None;
        }
        let snapshot = self.snapshot.take();
        if current == self.sequence {
            snapshot
        } else {
            None
        }
    }
    /// Called while the native clipboard is open, keeping validation and mutation atomic.
    pub fn write(
        &mut self,
        current: u32,
        write: impl FnOnce() -> ov_core::error::Result<u32>,
    ) -> ov_core::error::Result<()> {
        if self.sequence != current {
            self.snapshot = None;
            return Err(ov_core::error::Error::Injection(
                "clipboard changed; use Copy for the complete transcript".into(),
            ));
        }
        self.sequence = write()?;
        Ok(())
    }

    pub fn begin(&mut self, current: u32, snapshot: T) {
        if self.snapshot.is_none() || self.sequence != current {
            self.snapshot = Some(snapshot);
        }
        self.generation += 1;
        self.sequence = current;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_intervening_copy_is_not_overwritten() {
        let mut lease = Lease {
            snapshot: Some("original"),
            sequence: 2,
            generation: 1,
        };
        let mut clipboard = "user image";
        let result = lease.write(3, || {
            clipboard = "next piece";
            Ok(4)
        });
        assert!(result.is_err());
        assert_eq!(clipboard, "user image");
    }

    #[test]
    fn obsolete_restore_cannot_consume_consecutive_dictations_original() {
        let mut lease = Lease::default();
        lease.begin(1, "original image");
        lease.write(1, || Ok(2)).unwrap();
        lease.begin(2, "dictation A");
        lease.write(2, || Ok(3)).unwrap();
        assert_eq!(lease.take_for_restore(1, 3), None);
        assert_eq!(lease.take_for_restore(2, 3), Some("original image"));
    }

    #[test]
    fn new_sequence_even_with_identical_text_prevents_restore() {
        let mut lease = Lease {
            snapshot: Some("original"),
            sequence: 2,
            generation: 1,
        };
        assert_eq!(lease.take_for_restore(1, 3), None);
        assert_eq!(lease.snapshot, None);
    }

    #[test]
    fn consecutive_owned_writes_restore_the_original() {
        let mut lease = Lease {
            snapshot: None,
            sequence: 0,
            generation: 0,
        };
        lease.begin(1, "X");
        lease.sequence = 2;
        lease.begin(2, "dictation A");
        lease.sequence = 3;
        assert_eq!(lease.snapshot, Some("X"));
        assert_eq!(lease.generation, 2);
        lease.begin(4, "new user image");
        assert_eq!(lease.snapshot, Some("new user image"));
    }
}
