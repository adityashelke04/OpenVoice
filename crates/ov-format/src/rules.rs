//! The formatting rules, in pipeline order.
//!
//! Each rule is small, independent, and rewrites a [`Doc`] into a new [`Doc`].
//! Rules never read configuration directly — everything they need arrives in
//! [`Ctx`] — so a rule can be tested with three lines of setup.

use crate::dictionary::Dictionary;
use crate::doc::{Doc, Tok};
use crate::profile::{Capitalization, FillerLevel, Profile};

/// Everything a rule is allowed to know about the world.
pub struct Ctx<'a> {
    /// The active profile.
    pub profile: &'a Profile,
    /// The compiled vocabulary for this profile.
    pub dict: &'a Dictionary,
}

/// One stage of the pipeline.
pub trait Rule: Send + Sync {
    /// Stable name, shown in the debug panel's stage-by-stage trace.
    fn name(&self) -> &'static str;
    /// Rewrite the document.
    fn apply(&self, doc: Doc, ctx: &Ctx<'_>) -> Doc;
}

// ---------------------------------------------------------------------------
// 1. Fillers
// ---------------------------------------------------------------------------

/// Unambiguous disfluencies. Removing these is always safe.
const FILLERS_LIGHT: &[&str] = &["um", "uh", "erm", "uhm", "mhm", "hmm", "ah", "er"];

/// Discourse markers. Sometimes load-bearing, so only removed at
/// [`FillerLevel::Aggressive`].
const FILLERS_PHRASES: &[&[&str]] = &[
    &["you", "know"],
    &["i", "mean"],
    &["sort", "of"],
    &["kind", "of"],
];

/// Single-word discourse markers removed at the aggressive level.
const FILLERS_AGGRESSIVE: &[&str] = &["like", "basically", "actually", "literally"];

/// Removes disfluencies according to the profile's [`FillerLevel`].
pub struct StripFillers;

impl Rule for StripFillers {
    fn name(&self) -> &'static str {
        "fillers"
    }

    fn apply(&self, doc: Doc, ctx: &Ctx<'_>) -> Doc {
        let level = ctx.profile.fillers;
        if level == FillerLevel::Off {
            return doc;
        }
        let aggressive = level == FillerLevel::Aggressive;
        let mut out = Vec::with_capacity(doc.len());
        let mut i = 0;

        while i < doc.len() {
            if aggressive {
                if let Some(skip) = FILLERS_PHRASES
                    .iter()
                    .find(|p| doc.window(i, p.len()).is_some_and(|w| w == **p))
                    .map(|p| p.len())
                {
                    i += skip;
                    continue;
                }
            }
            let drop = doc.toks[i].as_word_lower().is_some_and(|w| {
                FILLERS_LIGHT.contains(&w.as_str())
                    || (aggressive && FILLERS_AGGRESSIVE.contains(&w.as_str()))
            });
            if !drop {
                out.push(doc.toks[i].clone());
            }
            i += 1;
        }
        // Removing a filler can strand the comma that followed it.
        strip_leading_punctuation(&mut out);
        Doc { toks: out }
    }
}

fn strip_leading_punctuation(toks: &mut Vec<Tok>) {
    while matches!(toks.first(), Some(Tok::Lit(s)) if matches!(s.as_str(), "," | "." | ";")) {
        toks.remove(0);
    }
}

// ---------------------------------------------------------------------------
// 2. Voice commands
// ---------------------------------------------------------------------------

/// Spoken phrase -> literal output, and whether it binds tightly to both
/// neighbours. Longest phrases are matched first.
///
/// The tight column is what separates `dash dash all` -> `--all` from
/// `- - all`, and `user dot name` -> `user.name` from `user. name`. Note that
/// "dot" and "period" both produce a full stop but differ in binding: one is part
/// of an identifier, the other ends a sentence.
const COMMANDS: &[(&[&str], &str, bool)] = &[
    (&["open", "parenthesis"], "(", false),
    (&["close", "parenthesis"], ")", false),
    (&["exclamation", "point"], "!", false),
    (&["exclamation", "mark"], "!", false),
    (&["question", "mark"], "?", false),
    (&["double", "quote"], "\"", false),
    (&["single", "quote"], "'", false),
    (&["open", "bracket"], "[", false),
    (&["close", "bracket"], "]", false),
    (&["open", "brace"], "{", false),
    (&["close", "brace"], "}", false),
    (&["open", "paren"], "(", false),
    (&["close", "paren"], ")", false),
    (&["dollar", "sign"], "$", false),
    (&["at", "sign"], "@", false),
    (&["full", "stop"], ".", false),
    (&["fat", "arrow"], "=>", false),
    (&["semicolon"], ";", false),
    (&["ampersand"], "&", false),
    (&["asterisk"], "*", false),
    (&["backtick"], "`", false),
    (&["backslash"], "\\", true),
    (&["underscore"], "_", true),
    (&["percent"], "%", false),
    (&["equals"], "=", false),
    (&["hyphen"], "-", true),
    (&["period"], ".", false),
    (&["comma"], ",", false),
    (&["colon"], ":", false),
    (&["arrow"], "->", false),
    (&["slash"], "/", true),
    (&["pipe"], "|", false),
    (&["dash"], "-", true),
    (&["star"], "*", false),
    (&["hash"], "#", false),
    (&["pound"], "#", false),
    (&["dot"], ".", true),
];

/// The escape hatch. "literally new line" emits the words, not a line break.
///
/// Without this, certain sentences become untypeable — you could never dictate
/// the phrase "press comma to continue" — which is infuriating in a way that is
/// hard to diagnose and easy to prevent.
const ESCAPE: &str = "literally";

/// Length of the run of consecutive dash words starting at `i`, if any.
fn dash_run(doc: &Doc, i: usize) -> Option<usize> {
    let is_dash = |n: usize| {
        doc.toks
            .get(n)
            .and_then(Tok::as_word_lower)
            .is_some_and(|w| w == "dash" || w == "hyphen")
    };
    if !is_dash(i) {
        return None;
    }
    let mut n = 1;
    while is_dash(i + n) {
        n += 1;
    }
    Some(n)
}

/// Interprets spoken punctuation and layout commands.
pub struct VoiceCommands;

impl Rule for VoiceCommands {
    fn name(&self) -> &'static str {
        "commands"
    }

    fn apply(&self, doc: Doc, ctx: &Ctx<'_>) -> Doc {
        if !ctx.profile.voice_commands {
            return doc;
        }
        let mut out: Vec<Tok> = Vec::with_capacity(doc.len());
        let mut i = 0;

        while i < doc.len() {
            // Escape: emit the next token verbatim, uninterpreted.
            if doc.toks[i].as_word_lower().as_deref() == Some(ESCAPE) {
                if let Some(next) = doc.toks.get(i + 1) {
                    out.push(next.clone());
                    i += 2;
                    continue;
                }
            }

            // "scratch that" discards everything dictated so far in this utterance.
            if doc.window(i, 2).is_some_and(|w| w == ["scratch", "that"]) {
                out.clear();
                i += 2;
                continue;
            }

            if doc.window(i, 2).is_some_and(|w| w == ["new", "paragraph"]) {
                out.push(Tok::Break);
                out.push(Tok::Break);
                i += 2;
                continue;
            }
            if doc.window(i, 2).is_some_and(|w| w == ["new", "line"]) {
                out.push(Tok::Break);
                i += 2;
                continue;
            }

            // Dash count is authoritative, and deliberately nothing else is.
            //
            // A run of two or more opens a command-line flag: a space before the
            // run, none inside, none after (`pods --all`). A lone dash is a hyphen
            // binding both sides (`well-known`, `--all-namespaces`).
            //
            // A smarter rule was tried and removed. `--rm -it` (two flags) and
            // `--all-namespaces` (one hyphenated flag) are the same shape in
            // speech — flag, word, dash, word — so no heuristic separates them
            // without a database of every CLI's flags. Chaining a short flag after
            // a long one is the case this rule gets wrong, and it loses to
            // predictability: a rule the user can learn and always be right about
            // beats one that is usually right and never trustworthy.
            //
            // One reliable exception: a single dash followed by a *single letter*
            // is a short flag (`git commit -m`, `ls -l`). Short flags are one
            // letter by convention, and a hyphen inside a word always joins parts
            // of two or more letters, so the two never collide.
            if let Some(run) = dash_run(&doc, i) {
                let short_flag = run == 1
                    && doc
                        .toks
                        .get(i + 1)
                        .and_then(Tok::as_word_lower)
                        .is_some_and(|w| w.chars().count() == 1 && w.chars().all(char::is_alphabetic));

                if run >= 2 || short_flag {
                    out.push(Tok::lit("-".repeat(run)));
                } else {
                    out.push(Tok::tight("-"));
                }
                i += run;
                continue;
            }

            if let Some((phrase, lit, tight)) = COMMANDS
                .iter()
                .find(|(p, _, _)| doc.window(i, p.len()).is_some_and(|w| w == **p))
            {
                out.push(if *tight { Tok::tight(*lit) } else { Tok::lit(*lit) });
                i += phrase.len();
                continue;
            }

            out.push(doc.toks[i].clone());
            i += 1;
        }
        Doc { toks: out }
    }
}

// ---------------------------------------------------------------------------
// 3. Dictionary
// ---------------------------------------------------------------------------

/// Replaces spoken forms with their correct written forms.
pub struct ApplyDictionary;

impl Rule for ApplyDictionary {
    fn name(&self) -> &'static str {
        "dictionary"
    }

    fn apply(&self, doc: Doc, ctx: &Ctx<'_>) -> Doc {
        if ctx.dict.is_empty() {
            return doc;
        }
        let mut out = Vec::with_capacity(doc.len());
        let mut i = 0;

        while i < doc.len() {
            let mut matched = false;
            // Longest match first, so "use effect" beats a hypothetical "use".
            for n in (1..=ctx.dict.max_words().min(doc.len() - i)).rev() {
                let Some(window) = doc.window(i, n) else {
                    continue;
                };
                if let Some(written) = ctx.dict.lookup(&window) {
                    out.push(Tok::lit(written));
                    i += n;
                    matched = true;
                    break;
                }
            }
            if !matched {
                out.push(doc.toks[i].clone());
                i += 1;
            }
        }
        Doc { toks: out }
    }
}

// ---------------------------------------------------------------------------
// 4. Case transforms
// ---------------------------------------------------------------------------

/// Identifier casing styles that can be spoken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Case {
    Camel,
    Pascal,
    Snake,
    Kebab,
    Screaming,
}

const CASE_TRIGGERS: &[(&[&str], Case)] = &[
    (&["screaming", "snake", "case"], Case::Screaming),
    (&["constant", "case"], Case::Screaming),
    (&["pascal", "case"], Case::Pascal),
    (&["camel", "case"], Case::Camel),
    (&["snake", "case"], Case::Snake),
    (&["kebab", "case"], Case::Kebab),
];

/// Upper bound on words a case transform will absorb.
///
/// The transform otherwise runs to the next punctuation mark, which is the right
/// behaviour for "camel case user name comma" but would swallow a whole paragraph
/// if the user forgot to punctuate. Six words is longer than any identifier anyone
/// should be writing, so the cap only ever fires on mistakes.
const CASE_MAX_WORDS: usize = 6;

/// Interprets "camel case user name" and friends.
pub struct CaseTransforms;

impl Rule for CaseTransforms {
    fn name(&self) -> &'static str {
        "case"
    }

    fn apply(&self, doc: Doc, ctx: &Ctx<'_>) -> Doc {
        if !ctx.profile.case_transforms {
            return doc;
        }
        let mut out = Vec::with_capacity(doc.len());
        let mut i = 0;

        while i < doc.len() {
            let trigger = CASE_TRIGGERS
                .iter()
                .find(|(p, _)| doc.window(i, p.len()).is_some_and(|w| w == **p));

            let Some((phrase, case)) = trigger else {
                out.push(doc.toks[i].clone());
                i += 1;
                continue;
            };

            // Absorb the following run of words, bounded by punctuation or the cap.
            let mut words = Vec::new();
            let mut j = i + phrase.len();
            while j < doc.len() && words.len() < CASE_MAX_WORDS {
                match doc.toks[j].as_word_lower() {
                    Some(w) => {
                        words.push(w);
                        j += 1;
                    }
                    None => break,
                }
            }

            if words.is_empty() {
                // "camel case" with nothing after it: leave the words alone rather
                // than silently deleting what the user said.
                out.push(doc.toks[i].clone());
                i += 1;
                continue;
            }

            out.push(Tok::lit(join_case(&words, *case)));
            i = j;
        }
        Doc { toks: out }
    }
}

fn join_case(words: &[String], case: Case) -> String {
    match case {
        Case::Snake => words.join("_"),
        Case::Kebab => words.join("-"),
        Case::Screaming => words.join("_").to_uppercase(),
        Case::Camel => words
            .iter()
            .enumerate()
            .map(|(i, w)| if i == 0 { w.clone() } else { capitalize(w) })
            .collect(),
        Case::Pascal => words.iter().map(|w| capitalize(w)).collect(),
    }
}

fn capitalize(w: &str) -> String {
    let mut cs = w.chars();
    match cs.next() {
        Some(c) => c.to_uppercase().collect::<String>() + cs.as_str(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// 5. Sentence capitalization
// ---------------------------------------------------------------------------

/// Capitalizes the first word of each sentence.
///
/// Only ever touches [`Tok::Word`]. A [`Tok::Lit`] is a resolved identifier —
/// capitalizing `useEffect` into `UseEffect` at the start of a sentence would turn
/// working code into a compile error, so literals are left strictly alone.
pub struct Capitalize;

impl Rule for Capitalize {
    fn name(&self) -> &'static str {
        "capitalize"
    }

    fn apply(&self, doc: Doc, ctx: &Ctx<'_>) -> Doc {
        if ctx.profile.capitalize != Capitalization::Sentence {
            return doc;
        }
        let mut toks = doc.toks;
        let mut at_sentence_start = true;

        for tok in &mut toks {
            match tok {
                Tok::Break => at_sentence_start = true,
                Tok::Lit(s) => {
                    // Terminal punctuation opens a sentence; anything else is
                    // content and therefore *closes* one. Forgetting the second
                    // half leaks the flag past a resolved identifier and
                    // capitalizes the following word: `useEffect Is a hook`.
                    at_sentence_start = matches!(s.as_str(), "." | "!" | "?");
                }
                // A tight token is always part of a word -- the full stop in
                // `user.name` does not start a sentence.
                Tok::Tight(_) => at_sentence_start = false,
                Tok::Word(w) => {
                    if at_sentence_start {
                        *w = capitalize(w);
                        at_sentence_start = false;
                    }
                }
            }
        }
        Doc { toks }
    }
}

// ---------------------------------------------------------------------------
// 6. Profile policy
// ---------------------------------------------------------------------------

/// Applies the profile's terminal-punctuation and leading-case policy.
pub struct ProfilePolicy;

impl Rule for ProfilePolicy {
    fn name(&self) -> &'static str {
        "profile"
    }

    fn apply(&self, doc: Doc, ctx: &Ctx<'_>) -> Doc {
        let mut toks = doc.toks;

        if ctx.profile.capitalize == Capitalization::ForceLower {
            // Lowercase only the first character of the first word: `Ls` is not a
            // command, but `Git` inside a sentence may be intentional.
            if let Some(Tok::Word(w)) = toks.first_mut() {
                let mut cs = w.chars();
                if let Some(c) = cs.next() {
                    *w = c.to_lowercase().collect::<String>() + cs.as_str();
                }
            }
        }

        if ctx.profile.end_period && !toks.is_empty() {
            let ends_terminal = toks
                .last()
                .map(|t| {
                    let s = t.text();
                    s.ends_with('.') || s.ends_with('!') || s.ends_with('?') || s.ends_with(':')
                })
                .unwrap_or(false);
            if !ends_terminal && !matches!(toks.last(), Some(Tok::Break)) {
                toks.push(Tok::lit("."));
            }
        }
        Doc { toks }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::{builtin_entries, Dictionary};
    use pretty_assertions::assert_eq;

    fn run(rule: &dyn Rule, profile: &Profile, input: &str) -> String {
        let dict = Dictionary::compile(&builtin_entries(), &profile.dictionaries);
        let ctx = Ctx {
            profile,
            dict: &dict,
        };
        rule.apply(Doc::parse(input), &ctx).render()
    }

    #[test]
    fn light_fillers_leave_discourse_markers_alone() {
        let p = Profile {
            fillers: FillerLevel::Light,
            ..Profile::default()
        };
        assert_eq!(
            run(&StripFillers, &p, "um so like the thing"),
            "so like the thing"
        );
    }

    #[test]
    fn aggressive_fillers_remove_phrases_too() {
        let p = Profile {
            fillers: FillerLevel::Aggressive,
            ..Profile::default()
        };
        assert_eq!(
            run(&StripFillers, &p, "so you know like the thing"),
            "so the thing"
        );
    }

    #[test]
    fn removing_a_filler_does_not_strand_its_comma() {
        let p = Profile {
            fillers: FillerLevel::Light,
            ..Profile::default()
        };
        assert_eq!(run(&StripFillers, &p, "um, hello there"), "hello there");
    }

    #[test]
    fn voice_commands_become_punctuation() {
        let p = Profile::default();
        assert_eq!(
            run(&VoiceCommands, &p, "hello comma world period"),
            "hello, world."
        );
        assert_eq!(
            run(&VoiceCommands, &p, "foo open paren bar close paren"),
            "foo (bar)"
        );
    }

    #[test]
    fn dashes_bind_into_command_line_flags() {
        // Regression: this rendered as `- - all namespaces`, which is not a
        // runnable command.
        let p = Profile::terminal();
        assert_eq!(
            run(&VoiceCommands, &p, "cube control get pods dash dash all namespaces"),
            "cube control get pods --all namespaces"
        );
        assert_eq!(run(&VoiceCommands, &p, "well dash known"), "well-known");
        // A hyphenated long flag: one dash inside the name stays a hyphen.
        assert_eq!(
            run(&VoiceCommands, &p, "cube control get pods dash dash all dash namespaces"),
            "cube control get pods --all-namespaces"
        );
    }

    #[test]
    fn single_letter_after_a_dash_is_a_short_flag() {
        let p = Profile::terminal();
        assert_eq!(
            run(&VoiceCommands, &p, "git commit dash m fix the overrun"),
            "git commit -m fix the overrun"
        );
        assert_eq!(run(&VoiceCommands, &p, "ls dash l"), "ls -l");
        // Multi-letter parts still hyphenate, so the two rules never collide.
        assert_eq!(run(&VoiceCommands, &p, "well dash known"), "well-known");
    }

    /// Documents the case the dash rule knowingly gets wrong.
    ///
    /// `--rm -it` is two flags but reads identically to a hyphenated one, so it
    /// comes out joined. Pinned as a test so the behaviour is a decision on record
    /// rather than a bug someone silently "fixes" — and so it fails loudly if a
    /// future rule ever makes it right.
    #[test]
    fn short_flag_after_long_flag_is_a_known_limitation() {
        let p = Profile::terminal();
        assert_eq!(
            run(&VoiceCommands, &p, "docker run dash dash rm dash it ubuntu"),
            "docker run --rm-it ubuntu"
        );
    }

    #[test]
    fn dot_builds_identifiers_while_period_ends_sentences() {
        let p = Profile::default();
        assert_eq!(run(&VoiceCommands, &p, "user dot name"), "user.name");
        assert_eq!(run(&VoiceCommands, &p, "done period next"), "done. next");
    }

    #[test]
    fn new_line_and_new_paragraph_break() {
        let p = Profile::default();
        assert_eq!(run(&VoiceCommands, &p, "one new line two"), "one\ntwo");
        assert_eq!(
            run(&VoiceCommands, &p, "one new paragraph two"),
            "one\n\ntwo"
        );
    }

    #[test]
    fn literally_escapes_a_command() {
        let p = Profile::default();
        assert_eq!(
            run(&VoiceCommands, &p, "press literally comma to continue"),
            "press comma to continue",
            "the escape hatch must make every phrase dictatable"
        );
    }

    #[test]
    fn scratch_that_discards_what_came_before() {
        let p = Profile::default();
        assert_eq!(
            run(&VoiceCommands, &p, "wrong words scratch that right words"),
            "right words"
        );
    }

    #[test]
    fn dictionary_prefers_the_longest_match() {
        let p = Profile::default();
        assert_eq!(
            run(&ApplyDictionary, &p, "call use effect here"),
            "call useEffect here"
        );
    }

    #[test]
    fn case_transforms_build_identifiers() {
        let p = Profile::default();
        assert_eq!(run(&CaseTransforms, &p, "camel case user name"), "userName");
        assert_eq!(
            run(&CaseTransforms, &p, "snake case user name"),
            "user_name"
        );
        assert_eq!(
            run(&CaseTransforms, &p, "kebab case user name"),
            "user-name"
        );
        assert_eq!(
            run(&CaseTransforms, &p, "pascal case user name"),
            "UserName"
        );
        assert_eq!(
            run(&CaseTransforms, &p, "screaming snake case max size"),
            "MAX_SIZE"
        );
    }

    #[test]
    fn case_transform_stops_at_punctuation() {
        let p = Profile::default();
        let doc = Doc::parse("camel case user name");
        let dict = Dictionary::default();
        let ctx = Ctx {
            profile: &p,
            dict: &dict,
        };
        // Simulate a comma already resolved by VoiceCommands.
        let mut d = CaseTransforms.apply(doc, &ctx);
        d.toks.push(Tok::lit(","));
        d.toks.push(Tok::word("then"));
        assert_eq!(d.render(), "userName, then");
    }

    #[test]
    fn case_transform_with_nothing_following_is_left_alone() {
        let p = Profile::default();
        assert_eq!(
            run(&CaseTransforms, &p, "camel case"),
            "camel case",
            "must not silently delete the user's words"
        );
    }

    #[test]
    fn capitalize_never_touches_resolved_identifiers() {
        let p = Profile::default();
        let dict = Dictionary::compile(&builtin_entries(), &p.dictionaries);
        let ctx = Ctx {
            profile: &p,
            dict: &dict,
        };
        let d = ApplyDictionary.apply(Doc::parse("use effect is a hook"), &ctx);
        assert_eq!(
            Capitalize.apply(d, &ctx).render(),
            "useEffect is a hook",
            "capitalizing an identifier would break the code"
        );
    }

    #[test]
    fn capitalize_starts_new_sentences() {
        let p = Profile::default();
        assert_eq!(
            run(&Capitalize, &p, "hello there. how are you"),
            "Hello there. How are you"
        );
    }

    #[test]
    fn terminal_profile_forces_lowercase_and_adds_no_period() {
        let p = Profile::terminal();
        assert_eq!(run(&ProfilePolicy, &p, "Git status"), "git status");
    }

    #[test]
    fn prose_profile_adds_a_terminal_period_only_when_missing() {
        let p = Profile::prose();
        assert_eq!(run(&ProfilePolicy, &p, "hello there"), "hello there.");
        assert_eq!(run(&ProfilePolicy, &p, "hello there!"), "hello there!");
    }
}
