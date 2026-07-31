//! Developer vocabulary: mapping what the model heard to what the user meant.
//!
//! Whisper transcribes `useEffect` as "use effect", `kubectl` as "cube control" or
//! "coup CTL", and `nginx` as "engine X". These are *phonetic* errors, so a literal
//! `HashMap<&str, &str>` catches only the spellings someone happened to enumerate.
//!
//! Two mechanisms address this:
//!
//! 1. **This dictionary**, which repairs the transcript after the fact.
//! 2. **Decode hints** ([`ov_core::ports::DecodeHint`]), which feed the same terms
//!    into the model's initial prompt so it gets them right in the first place.
//!
//! The second is strictly better, because the decoder still has the acoustic
//! evidence that post-processing has thrown away. The dictionary is the safety net,
//! not the primary mechanism.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One vocabulary term.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The correct written form, e.g. `useEffect`.
    pub written: String,
    /// Spoken forms to match, lowercase, possibly multi-word.
    ///
    /// Include the mistranscriptions actually observed, not just the "correct"
    /// pronunciation — "cube control" belongs here precisely because it is wrong.
    pub spoken: Vec<String>,
    /// Group this entry belongs to, e.g. `code` or `shell`.
    #[serde(default = "default_group")]
    pub group: String,
}

fn default_group() -> String {
    "code".into()
}

impl Entry {
    /// Build an entry from a written form and its spoken variants.
    pub fn new(written: &str, spoken: &[&str], group: &str) -> Self {
        Self {
            written: written.to_string(),
            spoken: spoken.iter().map(|s| s.to_lowercase()).collect(),
            group: group.to_string(),
        }
    }
}

/// A compiled, lookup-ready vocabulary.
#[derive(Debug, Clone, Default)]
pub struct Dictionary {
    /// Spoken phrase -> written form. Keys are lowercase, space-separated.
    by_phrase: HashMap<String, String>,
    /// Longest phrase in the map, in words. Bounds the match window so lookup cost
    /// does not grow with dictionary size.
    max_words: usize,
    /// Written forms in insertion order, for building decode hints.
    terms: Vec<String>,
}

impl Dictionary {
    /// Compile entries whose group is enabled into a lookup table.
    #[must_use]
    pub fn compile(entries: &[Entry], enabled_groups: &[String]) -> Self {
        let mut by_phrase = HashMap::new();
        let mut max_words = 0;
        let mut terms = Vec::new();

        for e in entries {
            if !enabled_groups.iter().any(|g| g == &e.group) {
                continue;
            }
            terms.push(e.written.clone());
            for phrase in &e.spoken {
                let norm = normalize(phrase);
                if norm.is_empty() {
                    continue;
                }
                max_words = max_words.max(norm.split(' ').count());
                // First writer wins, so earlier (user-defined) entries take
                // precedence over later (builtin) ones.
                by_phrase.entry(norm).or_insert_with(|| e.written.clone());
            }
        }
        Self {
            by_phrase,
            max_words,
            terms,
        }
    }

    /// Longest phrase length in words.
    #[must_use]
    pub fn max_words(&self) -> usize {
        self.max_words
    }

    /// Look up a run of lowercase words.
    #[must_use]
    pub fn lookup(&self, words: &[String]) -> Option<&str> {
        self.by_phrase.get(&words.join(" ")).map(String::as_str)
    }

    /// Number of distinct spoken phrases.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_phrase.len()
    }

    /// Whether the dictionary is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_phrase.is_empty()
    }

    /// Written forms, for packing into a decode hint.
    ///
    /// Whisper's initial prompt is capped at roughly 224 tokens, so the caller must
    /// choose a subset. Ranking by recency and frequency of actual use beats any
    /// fixed ordering, which is why this returns the raw list rather than trying to
    /// be clever here.
    #[must_use]
    pub fn terms(&self) -> &[String] {
        &self.terms
    }
}

/// Lowercase and collapse whitespace.
fn normalize(s: &str) -> String {
    s.split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The vocabulary shipped by default.
///
/// Kept deliberately small. A large shipped dictionary produces confident wrong
/// corrections in domains the user does not work in, which is worse than no
/// correction at all. The real value comes from terms the user adds, and later from
/// symbols scanned out of their own repositories.
#[must_use]
pub fn builtin_entries() -> Vec<Entry> {
    vec![
        // React / JS
        Entry::new("useEffect", &["use effect", "you seffect"], "code"),
        Entry::new("useState", &["use state"], "code"),
        Entry::new("useMemo", &["use memo", "use memmo"], "code"),
        Entry::new("useCallback", &["use callback"], "code"),
        Entry::new("npm", &["n p m", "enpiem"], "code"),
        Entry::new("TypeScript", &["type script"], "code"),
        Entry::new("JavaScript", &["java script"], "code"),
        Entry::new("Node.js", &["node j s", "node js"], "code"),
        Entry::new("JSON", &["jason", "j son"], "code"),
        // Infra / shell
        Entry::new(
            "kubectl",
            &["cube control", "cube c t l", "coup control", "cube cuddle"],
            "shell",
        ),
        Entry::new("nginx", &["engine x", "n g inx"], "shell"),
        Entry::new("PostgreSQL", &["postgres q l", "post gres"], "shell"),
        Entry::new("Kubernetes", &["kubernetes", "cuber netties"], "shell"),
        // Lowercase: in the shell group this is a binary name, and `Docker run`
        // is not a command. The capitalised product name belongs in prose, where
        // this entry does not apply.
        Entry::new("docker", &["docker"], "shell"),
        Entry::new("ssh", &["s s h"], "shell"),
        Entry::new("cd", &["c d", "see dee"], "shell"),
        // Note the absence of "get" as a spoken form for `git`. It is tempting --
        // "get status" is almost always meant as "git status" -- but "get" is far
        // too common a word to claim context-free, and claiming it turns
        // `kubectl get pods` into `kubectl git pods`. A shipped dictionary that
        // makes confident wrong corrections is worse than no dictionary; the user
        // can add "get" themselves if their own usage justifies it.
        Entry::new("git", &["git"], "shell"),
        // Rust
        Entry::new("async", &["a sync", "ay sink"], "code"),
        Entry::new("Vec", &["vec", "veck"], "code"),
        Entry::new("impl", &["imple", "im pill"], "code"),
        Entry::new("struct", &["struct"], "code"),
        Entry::new("enum", &["enum", "e num"], "code"),
        Entry::new("tokio", &["tokyo"], "code"),
        Entry::new("serde", &["sir day", "serde"], "code"),
        // General
        Entry::new("API", &["a p i"], "code"),
        Entry::new("CLI", &["c l i"], "code"),
        Entry::new("UI", &["u i"], "code"),
        Entry::new("SQL", &["sequel", "s q l"], "code"),
        Entry::new("OAuth", &["o auth", "oh auth"], "code"),
        Entry::new("regex", &["reg ex", "rejex"], "code"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict() -> Dictionary {
        Dictionary::compile(&builtin_entries(), &["code".into(), "shell".into()])
    }

    #[test]
    fn resolves_multiword_spoken_forms() {
        let d = dict();
        assert_eq!(
            d.lookup(&["use".into(), "effect".into()]),
            Some("useEffect")
        );
        assert_eq!(
            d.lookup(&["cube".into(), "control".into()]),
            Some("kubectl"),
            "mistranscriptions are the point of the dictionary"
        );
    }

    #[test]
    fn groups_gate_which_entries_compile() {
        let code_only = Dictionary::compile(&builtin_entries(), &["code".into()]);
        assert_eq!(
            code_only.lookup(&["use".into(), "effect".into()]),
            Some("useEffect")
        );
        assert_eq!(
            code_only.lookup(&["cube".into(), "control".into()]),
            None,
            "shell terms must not leak into a code-only profile"
        );
    }

    #[test]
    fn earlier_entries_win_so_users_can_override_builtins() {
        // "git" is claimed by a builtin, so this only passes if the user's entry
        // is consulted first.
        let mut entries = vec![Entry::new("hub", &["git"], "shell")];
        entries.extend(builtin_entries());
        let d = Dictionary::compile(&entries, &["shell".into()]);
        assert_eq!(d.lookup(&["git".into()]), Some("hub"));
    }

    #[test]
    fn max_words_bounds_the_match_window() {
        let d = dict();
        assert!(d.max_words() >= 3, "kubectl has a three-word spoken form");
        assert!(
            d.max_words() <= 6,
            "window must stay small enough to be cheap"
        );
    }

    #[test]
    fn unknown_phrases_return_none() {
        assert_eq!(dict().lookup(&["banana".into(), "sandwich".into()]), None);
    }
}
