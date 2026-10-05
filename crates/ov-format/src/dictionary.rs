//! Developer vocabulary: mapping what the model heard to what the user meant.
//!
//! Whisper transcribes `useEffect` as "use effect", `kubectl` as "cube control" or
//! "coup CTL", and `nginx` as "engine X". These are *phonetic* errors, so a literal
//! `HashMap<&str, &str>` catches only the spellings someone happened to enumerate.
//!
//! This dictionary repairs the transcript after the fact, and — as of a measured
//! A/B test on 2026-08-01 — it is the *primary* mechanism, not a safety net.
//!
//! The original design said the opposite. Feeding the same terms into Whisper's
//! `initial_prompt` was supposed to be strictly better, on the reasoning that the
//! decoder still has acoustic evidence that post-processing has thrown away. That
//! reasoning was sound and the conclusion was wrong. Same audio, same model, the
//! hint being the only difference:
//!
//! ```text
//! with hint:     camelCaseUserProfile ==NewUserProfile open paren close paren
//! without hint:  camel case user profile equals new user profile open paren close paren
//! ```
//!
//! A prompt full of camelCase identifiers teaches the model to *write* camelCase,
//! so it silently joins ordinary spoken words — including the very command words
//! ("camel case", "equals") the formatter needs to see. The damage is worse than
//! the problem it solves, because this dictionary can fix `use effect` anyway,
//! whereas nothing downstream can recover words the model already welded together.
//!
//! Hints remain available for genuinely unguessable proper nouns. For Whisper
//! they were off by default; for Parakeet they are the decoder's hotwords, which
//! bias the *spelling* of a word the audio already supports rather than seeding
//! a prompt, and which measured as a large gain. See `ov_core::ports::DecodeHint`
//! and `ov_asr::sherpa`.

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
    /// Offer this term to the decoder as an initial-prompt hint.
    ///
    /// Off unless asked for, and deliberately so — see the module doc. A prompt
    /// full of identifiers like `useEffect` teaches the model to weld ordinary
    /// spoken words together, which costs more than it buys. Proper nouns are
    /// the exception the doc leaves open: "Claude" and "Tauri" are ordinary
    /// words phonetically, so no amount of post-processing can distinguish them
    /// from "cloud" and "Tori" with confidence — the decoder has to be told they
    /// are candidates while it still has the audio.
    #[serde(default)]
    pub hint: bool,
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
            hint: false,
        }
    }

    /// A proper noun: also offered to the decoder as a hint. See [`Entry::hint`].
    pub fn proper(written: &str, spoken: &[&str], group: &str) -> Self {
        Self {
            hint: true,
            ..Self::new(written, spoken, group)
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

                // Also index the space-free form. Whisper often returns an
                // identifier already welded together but miscased -- `UseEffect`
                // for `useEffect` -- and without this the dictionary sees one
                // unknown token and leaves the wrong casing in place.
                let joined = norm.replace(' ', "");
                if joined != norm {
                    by_phrase.entry(joined).or_insert_with(|| e.written.clone());
                }

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

/// Proper nouns worth telling the decoder about, from entries marked
/// [`Entry::hint`].
///
/// Deliberately *not* [`Dictionary::terms`], which is every written form there
/// is. That set is mostly identifiers — `useEffect`, `kubectl` — and putting
/// those in the initial prompt is the thing the module doc records as measurably
/// harmful: the model starts welding ordinary spoken words together, including
/// the command words the formatter needs to see.
///
/// What is left is a short list of names that are ordinary words phonetically.
/// No amount of post-processing can tell "Claude" from "cloud" after the fact
/// with any confidence, because both are real; the decoder can, while it still
/// has the audio. Capped, because the prompt is a budget and a long one starts
/// behaving like the identifier list did.
#[must_use]
pub fn hint_terms(entries: &[Entry]) -> Vec<String> {
    const MAX: usize = 24;
    let mut seen = Vec::new();
    for e in entries.iter().filter(|e| e.hint) {
        if !seen.iter().any(|t: &String| t == &e.written) {
            seen.push(e.written.clone());
        }
        if seen.len() == MAX {
            break;
        }
    }
    seen
}

/// Terms offered to the decoder: the user's own dictionary first, then the
/// builtin proper nouns.
///
/// The user's entries all go, not just flagged ones: typing a term into the
/// Dictionary is already saying it matters. Builtins stay limited to
/// [`hint_terms`], for the reason given there. Bounded, because the decoder
/// leans toward every term it is given and a long list pulls on every word.
#[must_use]
pub fn decoder_terms(user: &[Entry], builtin: &[Entry]) -> Vec<String> {
    const MAX: usize = 48;
    let mut terms: Vec<String> = Vec::new();
    // An entry that lists its own written form as a spoken form says "leave this
    // word alone"; offering it would pull the decoder toward it.
    let offered = user
        .iter()
        .filter(|e| !e.spoken.iter().any(|s| s.eq_ignore_ascii_case(&e.written)))
        .map(|e| e.written.clone())
        .chain(hint_terms(builtin));
    for t in offered {
        if !terms.contains(&t) {
            terms.push(t);
        }
        if terms.len() == MAX {
            break;
        }
    }
    terms
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
        // The speech model hears "git" as "get" next to a git subcommand. These
        // pairs are not English ("get commit", "get checkout"), so they are safe
        // to claim everywhere; "get status" is, so it stays in the shell group.
        Entry::new("git commit", &["get commit"], "code"),
        Entry::new("git push", &["get push"], "code"),
        Entry::new("git pull", &["get pull"], "code"),
        Entry::new("git diff", &["get diff"], "code"),
        Entry::new("git checkout", &["get checkout"], "code"),
        Entry::new("git rebase", &["get rebase"], "code"),
        Entry::new("git stash", &["get stash"], "code"),
        Entry::new("git clone", &["get clone"], "code"),
        Entry::new("git status", &["get status"], "shell"),
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
        // Tooling and products people dictate constantly and Whisper has never
        // heard of. Every spoken form here is one that is not also an ordinary
        // English phrase — see the note on `git` above for why that line matters
        // more than coverage does.
        // Spoken forms below are the ones actually observed in dictation, not
        // plausible-looking guesses. An earlier version of this block guessed —
        // "tow ree" for Tauri — and caught nothing, while the form the model
        // really produces, "Tori", went straight through.
        Entry::proper("Vercel", &["versel", "ver cell", "verse elle"], "code"),
        Entry::proper("Tauri", &["tori", "taury", "torrey"], "code"),
        Entry::proper("OpenVoice", &["open voice"], "code"),
        Entry::new("GitHub", &["git hub"], "code"),
        Entry::new("pnpm", &["p n p m"], "code"),
        Entry::new("MCP", &["m c p"], "code"),
        Entry::new("SDK", &["s d k"], "code"),
        Entry::new("UX", &["u x"], "code"),
        Entry::new("CSS", &["c s s"], "code"),
        Entry::new("HTML", &["h t m l"], "code"),
        Entry::new("subagent", &["sub agent"], "code"),
        Entry::proper("Anthropic", &["anthropic"], "code"),
        // "Claude" is phonetically "cloud", and the transcripts show exactly
        // that: seventeen occurrences of "cloud", plus "plot code", "clawed" and
        // "claud".
        //
        // The two-word forms are the ones worth claiming. Nobody dictates "cloud
        // code" meaning weather, so the phrase can be rewritten with confidence
        // — whereas bare "cloud" is ordinary English ("cloud storage", "cloud
        // run") and a shipped dictionary has no business touching it. That is
        // the same line the `git`/`get` note above draws, and it is why the
        // phrase entries are here and a bare `cloud -> Claude` mapping is not:
        // anyone who says the name far more often than the word can add it in
        // Dictionary, which is what the user dictionary is for.
        //
        // The decoder hint is what gives bare "Claude" a chance on its own,
        // acoustically, before any of this runs.
        Entry::proper(
            "Claude Code",
            &[
                "cloud code",
                "plot code",
                "clod code",
                "cloud coat",
                "clawed code",
                "claw code",
                "clot code",
                // The speech model hears "Code" as the product name next to it.
                // "Claude Codex" is not a thing anyone dictates on purpose.
                "claude codex",
            ],
            "code",
        ),
        Entry::proper("Claude", &["clawed", "claud", "clode"], "code"),
        // Spoken forms are the ones the model produced from real dictation, as
        // above. These are offered to the decoder too: the model has never seen
        // "Docling" or "PaddleOCR", so it can only spell them if told they are
        // candidates while it still has the audio.
        Entry::proper("Codex", &["codex"], "code"),
        Entry::proper(
            "Docling",
            &["doc ling", "dock ling", "dockling", "dock link"],
            "code",
        ),
        Entry::proper(
            "PaddleOCR",
            &[
                "paddle ocr",
                "paddle o c r",
                "paddle o see are",
                "para lociar",
            ],
            "code",
        ),
        // "open tv" is what the model wrote for a spoken "Open CV". It is a real
        // phrase, but not one that anyone dictating code says.
        Entry::proper(
            "OpenCV",
            &["open c v", "open cv", "open see v", "open tv"],
            "code",
        ),
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
    fn resolves_identifiers_the_model_already_welded_together() {
        // Whisper returns `UseEffect` as a single miscased token. Without the
        // space-free index this leaves the wrong casing in the transcript.
        let d = dict();
        assert_eq!(d.lookup(&["useeffect".into()]), Some("useEffect"));
        assert_eq!(d.lookup(&["usestate".into()]), Some("useState"));
        // The spaced form still works.
        assert_eq!(
            d.lookup(&["use".into(), "effect".into()]),
            Some("useEffect")
        );
    }

    fn words(s: &str) -> Vec<String> {
        s.split(' ').map(str::to_owned).collect()
    }

    #[test]
    fn repairs_the_tool_names_heard_in_real_dictation() {
        // Every left-hand side here is a transcript the speech model really
        // produced from the owner's own history, not a plausible guess.
        let d = dict();
        for (heard, written) in [
            ("doc ling", "Docling"),
            ("dock ling", "Docling"),
            ("dockling", "Docling"),
            ("paddle ocr", "PaddleOCR"),
            ("paddle o c r", "PaddleOCR"),
            ("open c v", "OpenCV"),
            ("open cv", "OpenCV"),
            ("clawed code", "Claude Code"),
            ("claude codex", "Claude Code"),
            ("clot code", "Claude Code"),
        ] {
            assert_eq!(d.lookup(&words(heard)), Some(written), "heard {heard:?}");
        }
    }

    #[test]
    fn the_tool_names_are_offered_to_the_decoder() {
        let hints = hint_terms(&builtin_entries());
        for name in ["Claude Code", "Codex", "Docling", "PaddleOCR", "OpenCV"] {
            assert!(hints.iter().any(|h| h == name), "{name} missing: {hints:?}");
        }
    }

    #[test]
    fn a_users_own_terms_go_to_the_decoder_ahead_of_the_builtins() {
        // Someone who typed a term into their dictionary has told us it matters,
        // whether or not they thought to call it a proper noun.
        let user = vec![
            Entry::new("Zyma", &["zima"], "code"),
            Entry::new("Claude", &["cloud"], "code"),
        ];
        let terms = decoder_terms(&user, &builtin_entries());
        assert_eq!(terms[0], "Zyma");
        assert_eq!(terms[1], "Claude");
        assert!(terms.iter().any(|t| t == "Claude Code"), "{terms:?}");
        assert_eq!(
            terms.iter().filter(|t| *t == "Claude").count(),
            1,
            "a term the user and the builtins share is offered once"
        );
        assert!(
            !terms.iter().any(|t| t == "useEffect"),
            "builtin identifiers are still not offered"
        );
    }

    #[test]
    fn repairs_what_the_owner_actually_said_aloud() {
        // Read off the owner's own history: "Open CV" came back as "Open Tv",
        // "Paddle OCR" as "Para lociar", and "git commit" as "get commit".
        let d = dict();
        for (heard, written) in [
            ("open tv", "OpenCV"),
            ("para lociar", "PaddleOCR"),
            ("get commit", "git commit"),
            ("get push", "git push"),
            ("get checkout", "git checkout"),
            ("get status", "git status"),
        ] {
            assert_eq!(d.lookup(&words(heard)), Some(written), "heard {heard:?}");
        }
    }

    #[test]
    fn get_status_is_only_claimed_where_a_command_is_likely() {
        // "get status" is ordinary English ("get status updates"), so it is only
        // rewritten for the shell group. "get commit" is not, so it is always.
        let code_only = Dictionary::compile(&builtin_entries(), &["code".into()]);
        assert_eq!(code_only.lookup(&words("get status")), None);
        assert_eq!(code_only.lookup(&words("get commit")), Some("git commit"));
    }

    #[test]
    fn a_keep_it_as_written_entry_is_not_a_vocabulary_term() {
        // `cloud -> cloud` says "leave this word alone". Offering it to the decoder
        // would pull every "Claude" toward "cloud".
        let user = vec![
            Entry::new("cloud", &["cloud"], "code"),
            Entry::new("Zyma", &["zima"], "code"),
        ];
        let terms = decoder_terms(&user, &builtin_entries());
        assert!(!terms.iter().any(|t| t == "cloud"), "{terms:?}");
        assert!(terms.iter().any(|t| t == "Zyma"));
    }

    #[test]
    fn the_decoder_vocabulary_is_bounded() {
        let user: Vec<Entry> = (0..200)
            .map(|i| Entry::new(&format!("term{i}"), &["x"], "code"))
            .collect();
        assert!(decoder_terms(&user, &builtin_entries()).len() <= 48);
    }

    #[test]
    fn unknown_phrases_return_none() {
        assert_eq!(dict().lookup(&["banana".into(), "sandwich".into()]), None);
    }
}
