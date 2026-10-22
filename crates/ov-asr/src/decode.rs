//! Native-independent result validation and hotword retry decisions.
use ov_core::error::{Error, Result};
/// Whether `text` says one word three times running.
///
/// Nobody dictates that on purpose, and it is what a decoder biased toward a
/// word produces from noise. Used to throw a boosted decode away.
fn repeats_a_word(text: &str) -> bool {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .collect();
    words
        .windows(3)
        .any(|w| !w[0].is_empty() && w[0] == w[1] && w[1] == w[2])
}

pub(crate) fn recognizer_text(text: Option<String>) -> Result<String> {
    text.ok_or_else(|| {
        Error::Transcription("recognizer returned no result; retry the dictation".into())
    })
}

pub(crate) fn retry_without_hotwords(boosted: bool, text: &str) -> bool {
    if boosted && repeats_a_word(text) {
        tracing::warn!("boosted decode repeated a word; decoding without hotwords");
        true
    } else {
        false
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    struct Capture(Arc<Mutex<String>>);
    struct Fields<'a>(&'a mut String);
    impl tracing::field::Visit for Fields<'_> {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0.push_str(&format!("{}={value:?} ", field.name()));
        }
    }
    impl tracing::Subscriber for Capture {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            event.record(&mut Fields(&mut self.0.lock().unwrap()));
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }
    #[test]
    fn repeated_secret_triggers_retry_without_logging_transcript() {
        let log = Arc::new(Mutex::new(String::new()));
        tracing::subscriber::with_default(Capture(log.clone()), || {
            assert!(retry_without_hotwords(
                true,
                "secret_token_972 secret_token_972 secret_token_972"
            ));
            assert!(!retry_without_hotwords(
                false,
                "secret_token_972 secret_token_972 secret_token_972"
            ));
            assert!(!retry_without_hotwords(true, "ordinary sentence"));
        });
        let log = log.lock().unwrap();
        assert!(
            log.contains("boosted decode repeated a word"),
            "real warning must have been captured"
        );
        assert!(
            !log.contains("secret_token_972"),
            "raw transcript leaked into warning: {log}"
        );
    }
    #[test]
    fn missing_recognizer_result_is_an_error_but_empty_result_is_valid() {
        assert!(recognizer_text(None)
            .unwrap_err()
            .to_string()
            .contains("no result"));
        assert_eq!(recognizer_text(Some(String::new())).unwrap(), "");
    }
    #[test]
    fn repeated_word_decision_preserves_two_repetitions() {
        assert!(repeats_a_word("Claude Claude Claude"));
        assert!(repeats_a_word("so Claude, Claude, claude."));
        assert!(!repeats_a_word("Claude Claude"));
        assert!(!repeats_a_word("that that is odd"));
        assert!(!repeats_a_word(""));
    }
}
