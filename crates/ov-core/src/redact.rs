//! Removing secrets from anything that gets written down.
//!
//! `PrivacyConfig::redact_patterns` has shipped with sensible defaults since v0.1
//! — OpenAI keys, GitHub tokens, AWS access key ids — and until now nothing read
//! it. The field was serialised, migrated and validated, and had no effect. This
//! module is what makes it true.
//!
//! # What this does and does not protect
//!
//! Redaction applies to the **stored** copy: the history database and the log. It
//! deliberately does not alter the text delivered to the application the user was
//! dictating into, because that text is the thing they asked for, and silently
//! mangling it would be a data-loss bug wearing a privacy costume. By the time
//! this runs the transcript has already been injected.
//!
//! So the threat model is narrow and worth stating plainly: it stops a dictated
//! secret from being *retained* — searchable in history, readable in a log someone
//! attaches to a bug report months later — not from reaching the editor it was
//! dictated into.
//!
//! # Why a bad pattern is not fatal
//!
//! These patterns are hand-edited in a TOML file. A user who mistypes one has made
//! their history slightly less redacted; a user whose app refuses to start because
//! of it has lost dictation entirely. Invalid patterns are therefore reported and
//! skipped, never fatal — and reported *by name*, since "invalid regex" without the
//! offending pattern is unactionable.

use regex::Regex;

/// What replaces a match. Fixed rather than configurable: a redaction marker that
/// varies between installs makes history harder to reason about, and there is no
/// use case for choosing it.
const MARKER: &str = "[redacted]";

/// A compiled set of redaction patterns.
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    patterns: Vec<Regex>,
}

impl Redactor {
    /// Compile `patterns`, returning the redactor and a message per rejected
    /// pattern.
    ///
    /// Returning the errors rather than logging them keeps this module free of a
    /// logging dependency and lets the caller decide how loud to be — the CLI and
    /// the app want different volumes.
    #[must_use]
    pub fn compile(patterns: &[String]) -> (Self, Vec<String>) {
        let mut compiled = Vec::new();
        let mut errors = Vec::new();

        for p in patterns {
            if p.trim().is_empty() {
                continue;
            }
            match Regex::new(p) {
                Ok(re) => compiled.push(re),
                Err(e) => errors.push(format!("ignoring invalid redaction pattern {p:?}: {e}")),
            }
        }

        (Self { patterns: compiled }, errors)
    }

    /// Whether anything would be redacted at all.
    ///
    /// Lets a caller skip allocating a new `String` per utterance in the common
    /// case of an empty pattern list.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Replace every match with `[redacted]`.
    #[must_use]
    pub fn apply(&self, text: &str) -> String {
        let mut out = text.to_string();
        for re in &self.patterns {
            // `Cow` avoids a copy when a pattern does not match, which is the
            // usual case: most utterances contain no secrets at all.
            if let std::borrow::Cow::Owned(replaced) = re.replace_all(&out, MARKER) {
                out = replaced;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PrivacyConfig;

    fn shipped() -> Redactor {
        let (r, errors) = Redactor::compile(&PrivacyConfig::default().redact_patterns);
        assert!(errors.is_empty(), "shipped defaults must all compile");
        r
    }

    #[test]
    fn the_shipped_defaults_all_compile() {
        // If a default pattern were invalid, redaction would silently do nothing
        // for every user who never edited their config -- which is all of them.
        assert!(!shipped().is_empty());
    }

    #[test]
    fn redacts_an_openai_key() {
        let out = shipped().apply("my key is sk-abcdefghijklmnopqrstuvwxyz012345 ok");
        assert!(!out.contains("abcdefghijklmnopqrstuvwxyz"), "{out}");
        assert!(out.contains(MARKER), "{out}");
    }

    #[test]
    fn redacts_a_github_token() {
        let out = shipped().apply("ghp_abcdefghijklmnopqrstuvwxyz0123456789");
        assert_eq!(out, MARKER);
    }

    #[test]
    fn redacts_an_aws_access_key_id() {
        let out = shipped().apply("AKIAIOSFODNN7EXAMPLE is the id");
        assert!(out.starts_with(MARKER), "{out}");
    }

    #[test]
    fn leaves_ordinary_speech_completely_alone() {
        // The failure that would matter most: a redactor that eats normal text
        // corrupts history for everyone while protecting nobody.
        let text = "So we need to call useEffect here, then return null";
        assert_eq!(shipped().apply(text), text);
    }

    #[test]
    fn redacts_every_occurrence_not_just_the_first() {
        let out = shipped()
            .apply("ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa and ghp_bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        assert_eq!(out, format!("{MARKER} and {MARKER}"));
    }

    #[test]
    fn an_invalid_pattern_is_skipped_and_reported_by_name() {
        let (r, errors) = Redactor::compile(&["(unclosed".to_string(), "secret".to_string()]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("(unclosed"), "{}", errors[0]);
        // The valid one still works: one bad pattern must not disable the rest.
        assert_eq!(r.apply("a secret here"), format!("a {MARKER} here"));
    }

    #[test]
    fn an_empty_pattern_list_is_a_no_op() {
        let (r, errors) = Redactor::compile(&[]);
        assert!(errors.is_empty());
        assert!(r.is_empty());
        assert_eq!(r.apply("anything at all"), "anything at all");
    }

    #[test]
    fn blank_patterns_are_ignored_rather_than_matching_everything() {
        // An empty regex matches at every position, so treating it as a real
        // pattern would replace the entire transcript with markers.
        let (r, errors) = Redactor::compile(&["".to_string(), "   ".to_string()]);
        assert!(errors.is_empty());
        assert!(r.is_empty());
        assert_eq!(r.apply("hello"), "hello");
    }

    #[test]
    fn redaction_is_idempotent() {
        // History replay and re-formatting both re-run over stored text; a second
        // pass must not redact the marker itself into something else.
        let r = shipped();
        let once = r.apply("ghp_abcdefghijklmnopqrstuvwxyz0123456789");
        assert_eq!(r.apply(&once), once);
    }
}
