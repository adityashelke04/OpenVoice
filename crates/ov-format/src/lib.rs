//! # ov-format — turning transcripts into developer text
//!
//! This crate is the product. The ASR model is a swappable commodity; what makes
//! OpenVoice worth using instead of raw Whisper is everything that happens *after*
//! the model returns a string.
//!
//! Raw Whisper output is not usable for developers. It writes `use effect` instead
//! of `useEffect`, `cube control` instead of `kubectl`, spells out "open
//! parenthesis" as three words, and capitalizes shell commands into oblivion. Those
//! are not model bugs — they are the correct transcription of what was said. Fixing
//! them is a text-processing problem, and text processing is deterministic,
//! testable, and improvable one rule at a time.
//!
//! ## Purity
//!
//! Like [`ov_core`](https://docs.rs/ov-core), this crate touches no OS, no clock, no
//! IO. CI compiles it for `wasm32-unknown-unknown` to prove it. That is what makes
//! the golden-file suite run in milliseconds, which is what makes it realistic to
//! tune these rules daily.
//!
//! ## Order
//!
//! ```text
//! parse -> fillers -> commands -> dictionary -> case -> capitalize -> profile
//! ```
//!
//! The order is load-bearing. Fillers go first so a stray "um" cannot break a
//! multi-word command match. Commands run before the dictionary so spoken
//! punctuation becomes literal tokens the dictionary will not try to interpret.
//! Capitalization runs late, after identifiers have been resolved into
//! [`doc::Tok::Lit`] tokens it is forbidden to touch — capitalizing `useEffect` into
//! `UseEffect` would turn working code into a compile error.
//!
//! ## Example
//!
//! ```
//! use ov_format::{Formatter, profile::Profile};
//!
//! let f = Formatter::with_builtins(Profile::editor());
//! assert_eq!(f.format("call use effect comma then return"), "Call useEffect, then return");
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::all)]

pub mod dictionary;
pub mod doc;
pub mod profile;
pub mod rules;

use dictionary::{builtin_entries, Dictionary, Entry};
use doc::Doc;
use profile::Profile;
use rules::{
    ApplyDictionary, Capitalize, CaseTransforms, Ctx, ProfilePolicy, Rule, StripFillers,
    VoiceCommands,
};

/// Output of one pipeline stage, for the debug panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageOutput {
    /// Rule name.
    pub stage: &'static str,
    /// Rendered text after this rule ran.
    pub text: String,
}

/// The result of formatting, including the per-stage trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Formatted {
    /// Final text to inject.
    pub text: String,
    /// Text after each stage, in order.
    ///
    /// This exists because "the formatter did something weird" is otherwise an
    /// unfalsifiable bug report. With the trace, the offending rule is obvious in
    /// about ten seconds, which is the difference between a rule set that keeps
    /// improving and one nobody dares touch.
    pub trace: Vec<StageOutput>,
}

/// A configured formatting pipeline.
pub struct Formatter {
    profile: Profile,
    dict: Dictionary,
    rules: Vec<Box<dyn Rule>>,
}

impl Formatter {
    /// Build a formatter from a profile and an explicit vocabulary.
    #[must_use]
    pub fn new(profile: Profile, entries: &[Entry]) -> Self {
        let dict = Dictionary::compile(entries, &profile.dictionaries);
        Self {
            profile,
            dict,
            rules: default_rules(),
        }
    }

    /// Build a formatter using the vocabulary shipped with OpenVoice.
    #[must_use]
    pub fn with_builtins(profile: Profile) -> Self {
        Self::new(profile, &builtin_entries())
    }

    /// Format a raw transcript.
    #[must_use]
    pub fn format(&self, raw: &str) -> String {
        self.format_traced(raw).text
    }

    /// Format a raw transcript, recording the output of every stage.
    #[must_use]
    pub fn format_traced(&self, raw: &str) -> Formatted {
        let ctx = Ctx {
            profile: &self.profile,
            dict: &self.dict,
        };
        let mut doc = Doc::parse(raw);
        let mut trace = Vec::with_capacity(self.rules.len() + 1);
        trace.push(StageOutput {
            stage: "parse",
            text: doc.render(),
        });

        for rule in &self.rules {
            doc = rule.apply(doc, &ctx);
            trace.push(StageOutput {
                stage: rule.name(),
                text: doc.render(),
            });
        }
        Formatted {
            text: doc.render(),
            trace,
        }
    }

    /// The active profile.
    #[must_use]
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// Vocabulary terms, for building an ASR decode hint.
    #[must_use]
    pub fn hint_terms(&self) -> &[String] {
        self.dict.terms()
    }
}

/// The standard rule order. See the module docs for why it is what it is.
fn default_rules() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(StripFillers),
        Box::new(VoiceCommands),
        Box::new(ApplyDictionary),
        Box::new(CaseTransforms),
        Box::new(Capitalize),
        Box::new(ProfilePolicy),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn editor_profile_end_to_end() {
        let f = Formatter::with_builtins(Profile::editor());
        assert_eq!(
            f.format("um so we need to call use effect here comma then return null"),
            "So we need to call useEffect here, then return null"
        );
    }

    #[test]
    fn terminal_profile_produces_a_runnable_command() {
        let f = Formatter::with_builtins(Profile::terminal());
        assert_eq!(
            f.format("cube control get pods"),
            "kubectl get pods",
            "no capital, no trailing period, correct binary name"
        );
        // Regression: the builtin dictionary once mapped "get" -> "git", which
        // silently corrupted the line above into `kubectl git pods`. A common
        // English word must not be claimed context-free.
        assert_eq!(f.format("get the pods"), "get the pods");
    }

    #[test]
    fn prose_profile_cleans_and_punctuates() {
        let f = Formatter::with_builtins(Profile::prose());
        assert_eq!(
            f.format("um so basically you know the deploy is done"),
            "So the deploy is done."
        );
    }

    #[test]
    fn trace_records_every_stage() {
        let f = Formatter::with_builtins(Profile::editor());
        let out = f.format_traced("um call use effect");
        let names: Vec<_> = out.trace.iter().map(|s| s.stage).collect();
        assert_eq!(
            names,
            vec![
                "parse",
                "fillers",
                "commands",
                "dictionary",
                "case",
                "capitalize",
                "profile"
            ]
        );
        assert_eq!(
            out.trace[1].text, "call use effect",
            "fillers removed the 'um'"
        );
        assert_eq!(
            out.trace[3].text, "call useEffect",
            "dictionary resolved the identifier"
        );
    }

    #[test]
    fn empty_input_stays_empty() {
        let f = Formatter::with_builtins(Profile::editor());
        assert_eq!(f.format(""), "");
        assert_eq!(f.format("   "), "");
    }

    #[test]
    fn an_utterance_of_pure_filler_empties_out() {
        // The session machine relies on this: empty formatter output means
        // "nothing to inject", and it finishes the session instead of pasting air.
        let f = Formatter::with_builtins(Profile::editor());
        assert_eq!(f.format("um uh erm"), "");
    }

    #[test]
    fn identifiers_survive_sentence_capitalization() {
        let f = Formatter::with_builtins(Profile::editor());
        assert_eq!(
            f.format("use effect runs after render"),
            "useEffect runs after render",
            "an identifier at the start of a sentence must not be capitalized"
        );
    }

    #[test]
    fn formatting_is_idempotent_on_already_clean_text() {
        // Re-running the formatter over its own output must not drift. History
        // replay depends on this, and drift here would be invisible in production.
        let f = Formatter::with_builtins(Profile::editor());
        let once = f.format("call use effect here");
        assert_eq!(f.format(&once), once);
    }
}
