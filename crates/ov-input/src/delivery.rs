//! Decisions made before each native input operation.

use ov_core::error::Result;
use ov_core::types::ForegroundApp;

/// Narrow native boundary so cancellation and clipboard scheduling are deterministic in tests.
pub(crate) trait PasteIo {
    fn check(&mut self) -> Result<()>;
    fn write(&mut self, text: &str) -> Result<()>;
    fn paste(&mut self) -> Result<()>;
    fn wait(&mut self);
}

pub(crate) fn paste(text: &str, target: &ForegroundApp, io: &mut impl PasteIo) -> Result<()> {
    let chunks = pieces(text, target);
    for (index, piece) in chunks.iter().enumerate() {
        if index > 0 {
            io.wait();
        }
        io.check()?;
        io.write(piece)?;
        io.check()?;
        io.paste()?;
    }
    if chunks.len() > 1 {
        io.wait();
        io.check()?;
        io.write(text)?;
    }
    Ok(())
}

pub(crate) fn unicode_pieces(text: &str) -> Vec<Vec<u16>> {
    let mut pieces = Vec::new();
    let mut chunk = Vec::with_capacity(4);
    for ch in text.chars() {
        if chunk.len() + ch.len_utf16() > 4 {
            pieces.push(std::mem::take(&mut chunk));
        }
        chunk.extend_from_slice(ch.encode_utf16(&mut [0; 2]));
    }
    if !chunk.is_empty() {
        pieces.push(chunk);
    }
    pieces
}

pub(crate) fn unicode(text: &str, mut send: impl FnMut(&[u16]) -> Result<()>) -> Result<()> {
    for piece in unicode_pieces(text) {
        send(&piece)?;
    }
    Ok(())
}

pub(crate) fn allowed(original: &ForegroundApp, current: &ForegroundApp, cancelled: bool) -> bool {
    !cancelled && original.window.is_some() && original.window == current.window
}

pub(crate) fn pieces<'a>(text: &'a str, target: &ForegroundApp) -> Vec<&'a str> {
    if target.exe.eq_ignore_ascii_case("claude.exe") {
        crate::chunk::paste_chunks(text, 700, 2)
    } else if text.is_empty() {
        Vec::new()
    } else {
        vec![text]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ownership::Lease;
    use ov_core::error::Error;

    struct Fake {
        lease: Lease<&'static str>,
        clipboard: String,
        sequence: u32,
        pasted: Vec<String>,
        waits: usize,
        copy_at_wait: Option<usize>,
        cancel_at_wait: Option<usize>,
        blur_at_wait: Option<usize>,
        focused: bool,
        cancelled: bool,
        open_fails: bool,
        paste_fails: bool,
    }

    impl Default for Fake {
        fn default() -> Self {
            Self {
                lease: Lease {
                    snapshot: Some("original"),
                    sequence: 1,
                    generation: 1,
                },
                clipboard: "original".into(),
                sequence: 1,
                pasted: vec![],
                waits: 0,
                copy_at_wait: None,
                cancel_at_wait: None,
                blur_at_wait: None,
                focused: true,
                cancelled: false,
                open_fails: false,
                paste_fails: false,
            }
        }
    }

    impl PasteIo for Fake {
        fn check(&mut self) -> Result<()> {
            if self.cancelled || !self.focused {
                Err(Error::Injection("cancelled or unfocused".into()))
            } else {
                Ok(())
            }
        }
        fn write(&mut self, text: &str) -> Result<()> {
            if self.open_fails {
                return Err(Error::Injection("clipboard busy".into()));
            }
            self.lease.write(self.sequence, || {
                self.clipboard = text.into();
                self.sequence += 1;
                Ok(self.sequence)
            })
        }
        fn paste(&mut self) -> Result<()> {
            if self.paste_fails {
                return Err(Error::Injection("paste blocked".into()));
            }
            self.pasted.push(self.clipboard.clone());
            Ok(())
        }
        fn wait(&mut self) {
            self.waits += 1;
            if self.copy_at_wait == Some(self.waits) {
                self.clipboard = "user image".into();
                self.sequence += 1;
            }
            if self.cancel_at_wait == Some(self.waits) {
                self.cancelled = true;
            }
            if self.blur_at_wait == Some(self.waits) {
                self.focused = false;
            }
        }
    }

    fn claude() -> ForegroundApp {
        ForegroundApp {
            exe: "claude.exe".into(),
            ..Default::default()
        }
    }

    #[test]
    fn image_copied_between_pieces_survives() {
        let mut io = Fake {
            copy_at_wait: Some(1),
            ..Default::default()
        };
        assert!(paste(&"a".repeat(701), &claude(), &mut io).is_err());
        assert_eq!(io.clipboard, "user image");
        assert_eq!(io.pasted.len(), 1);
    }

    #[test]
    fn image_copied_after_final_piece_survives_full_text_recovery() {
        let mut io = Fake {
            copy_at_wait: Some(2),
            ..Default::default()
        };
        assert!(paste(&"a".repeat(701), &claude(), &mut io).is_err());
        assert_eq!(io.clipboard, "user image");
        assert_eq!(io.pasted.len(), 2);
    }

    #[test]
    fn escape_during_final_wait_stops_full_text_write() {
        let mut io = Fake {
            cancel_at_wait: Some(2),
            ..Default::default()
        };
        assert!(paste(&"a".repeat(701), &claude(), &mut io).is_err());
        assert_eq!(io.clipboard, "a");
    }

    #[test]
    fn unicode_chunks_never_leave_half_a_surrogate_pair() {
        assert_eq!(
            unicode_pieces("abc😀"),
            vec![vec![97, 98, 99], vec![0xd83d, 0xde00]]
        );
    }

    #[test]
    fn escape_during_short_unicode_stops_later_batches() {
        let target: ForegroundApp = serde_json::from_str(
            r#"{"exe":"editor.exe","title":"one","window":{"handle":11,"process_id":5}}"#,
        )
        .unwrap();
        let mut sent = Vec::new();
        let result = unicode("abcd😀efgh", |batch| {
            if !allowed(&target, &target, !sent.is_empty()) {
                return Err(Error::Injection("Escape".into()));
            }
            sent.extend_from_slice(batch);
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(sent, vec![97, 98, 99, 100]);
    }

    #[test]
    fn cancelled_before_injection_has_no_side_effects() {
        let mut io = Fake {
            cancelled: true,
            ..Default::default()
        };
        assert!(paste("complete transcript", &claude(), &mut io).is_err());
        assert_eq!(io.clipboard, "original");
        assert!(io.pasted.is_empty());
    }

    #[test]
    fn focus_change_and_escape_between_pieces_stop_all_later_input() {
        for mut io in [
            Fake {
                cancel_at_wait: Some(1),
                ..Default::default()
            },
            Fake {
                blur_at_wait: Some(1),
                ..Default::default()
            },
        ] {
            assert!(paste(&"a".repeat(701), &claude(), &mut io).is_err());
            assert_eq!(io.pasted, vec!["a".repeat(700)]);
        }
    }

    #[test]
    fn clipboard_open_failure_preserves_original_and_sends_nothing() {
        let mut io = Fake {
            open_fails: true,
            ..Default::default()
        };
        assert!(paste("complete transcript", &claude(), &mut io).is_err());
        assert_eq!(io.clipboard, "original");
        assert!(io.pasted.is_empty());
    }

    #[test]
    fn failed_or_silently_rejected_ordinary_paste_keeps_complete_text() {
        for fails in [true, false] {
            let mut io = Fake {
                paste_fails: fails,
                ..Default::default()
            };
            let text = "long transcript\n".repeat(200);
            let _ = paste(&text, &ForegroundApp::default(), &mut io);
            assert_eq!(io.clipboard, text);
            assert_eq!(io.waits, 0);
        }
    }

    #[test]
    fn final_chunk_consumed_before_complete_text_is_restored() {
        let mut io = Fake::default();
        let text = "😀 line\n".repeat(200);
        paste(&text, &claude(), &mut io).unwrap();
        assert_eq!(io.pasted.concat(), text);
        assert_eq!(io.clipboard, text);
        assert_eq!(io.waits, io.pasted.len());
    }

    #[test]
    fn changed_or_unknown_window_never_receives_input() {
        let original: ForegroundApp = serde_json::from_str(
            r#"{"exe":"Code.exe","title":"one","window":{"handle":11,"process_id":5}}"#,
        )
        .unwrap();
        let other: ForegroundApp = serde_json::from_str(
            r#"{"exe":"Code.exe","title":"two","window":{"handle":12,"process_id":5}}"#,
        )
        .unwrap();
        assert!(!allowed(&original, &other, false));
        assert!(!allowed(
            &ForegroundApp::default(),
            &ForegroundApp::default(),
            false
        ));
    }

    #[test]
    fn ordinary_targets_receive_one_paste() {
        let text = "one\ntwo\n".repeat(200);
        let target = ForegroundApp {
            exe: "notepad.exe".into(),
            ..Default::default()
        };
        assert_eq!(pieces(&text, &target), vec![text.as_str()]);
    }
}
