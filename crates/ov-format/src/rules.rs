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
// 0. Degenerate output
// ---------------------------------------------------------------------------

/// A character repeated more than this many times is a decoding artefact.
///
/// Three allows a legitimate ellipsis and an emphatic `!!!`; nothing a person
/// dictates needs four of the same character in a row.
const MAX_CHAR_RUN: usize = 3;

/// Consecutive repeats of the same phrase beyond this are a repetition loop.
///
/// Three is deliberately permissive — "no no no" and a genuinely repeated phrase
/// are real speech — while still catching `Hello Hello Hello Hello Hello`.
const MAX_WORD_RUN: usize = 3;

/// Longest phrase length checked for looping.
///
/// Loops repeat phrases, not just words: `1, 2, 3, 4, 1, 2, 3, 4` and
/// `good job, good job, good job` are the shape this actually takes. Checking only
/// single tokens misses all of them.
const MAX_NGRAM: usize = 4;

/// Collapses the repetition loops and hallucinations Whisper produces.
///
/// The decoder is configured to avoid these (beam search, repetition penalty,
/// compression-ratio fallback), and that is where the real fix lives. This rule
/// exists because "mostly" is not good enough when the output goes straight into
/// the user's editor: a single `kkkkkkkkkkkkkkkkkkkk` pasted into source code is
/// worse than a hundred slightly-awkward transcripts.
///
/// It runs first, so every later rule sees sane input.
pub struct CollapseRepeats;

impl Rule for CollapseRepeats {
    fn name(&self) -> &'static str {
        "repeats"
    }

    fn apply(&self, doc: Doc, _ctx: &Ctx<'_>) -> Doc {
        let mut out: Vec<Tok> = Vec::with_capacity(doc.len());

        for tok in doc.toks {
            // Squash runs of one character *inside* a token: `?????????` -> `???`.
            out.push(match &tok {
                Tok::Word(s) => Tok::Word(squash_chars(s)),
                Tok::Lit(s) => Tok::Lit(squash_chars(s)),
                other => other.clone(),
            });

            // Then, if the tail is now one repeat too many of any short phrase,
            // drop that repeat. Done incrementally so a long loop is trimmed as it
            // arrives rather than needing a second pass.
            for n in 1..=MAX_NGRAM {
                let span = n * (MAX_WORD_RUN + 1);
                if out.len() < span {
                    break;
                }
                let tail = &out[out.len() - span..];
                let first = &tail[..n];
                let looping = tail
                    .chunks_exact(n)
                    .all(|chunk| chunk.iter().zip(first).all(|(a, b)| same_text(a, b)));
                if looping {
                    out.truncate(out.len() - n);
                    break;
                }
            }
        }

        Doc { toks: out }
    }
}

/// Reduce any run of the same character to at most [`MAX_CHAR_RUN`].
fn squash_chars(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last: Option<char> = None;
    let mut run = 0usize;
    for c in s.chars() {
        if Some(c) == last {
            run += 1;
        } else {
            last = Some(c);
            run = 1;
        }
        if run <= MAX_CHAR_RUN {
            out.push(c);
        }
    }
    out
}

fn same_text(a: &Tok, b: &Tok) -> bool {
    match (a, b) {
        (Tok::Break, Tok::Break) => true,
        (Tok::Break, _) | (_, Tok::Break) => false,
        _ => a.text().eq_ignore_ascii_case(b.text()),
    }
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

/// How many tokens starting at `i` would be consumed by command interpretation,
/// if any command matches there at all.
///
/// The escape hatch uses this to protect a *whole* command phrase, not just its
/// first word. Voice commands are not all single words: a dash run is variable
/// length, and two-word `COMMANDS` entries exist whose second word is *also* a
/// standalone single-word entry (`"fat arrow"` -> `=>`, but bare `"arrow"` ->
/// `->`). Escaping only one word after "literally" left the second word of a
/// two-word phrase exposed to the very next loop iteration: `literally fat
/// arrow` escaped `fat` and then re-interpreted the bare `arrow` as `->`,
/// rendering `fat ->` instead of the literal words `fat arrow`.
fn command_span(doc: &Doc, i: usize) -> usize {
    if doc.window(i, 2).is_some_and(|w| w == ["scratch", "that"]) {
        return 2;
    }
    if doc.window(i, 2).is_some_and(|w| w == ["new", "paragraph"]) {
        return 2;
    }
    if doc.window(i, 2).is_some_and(|w| w == ["new", "line"]) {
        return 2;
    }
    if let Some(run) = dash_run(doc, i) {
        return run;
    }
    if let Some((phrase, _, _)) = COMMANDS
        .iter()
        .find(|(p, _, _)| doc.window(i, p.len()).is_some_and(|w| w == **p))
    {
        return phrase.len();
    }
    1
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
            // Escape: emit the following command phrase verbatim, uninterpreted.
            // Must cover the *whole* phrase a command there would have consumed
            // (see `command_span`), not just one word -- otherwise the tail of a
            // multi-word command is left for the next iteration to reinterpret.
            if doc.toks[i].as_word_lower().as_deref() == Some(ESCAPE) && i + 1 < doc.len() {
                let span = command_span(&doc, i + 1).min(doc.len() - (i + 1));
                for tok in &doc.toks[i + 1..i + 1 + span] {
                    out.push(tok.clone());
                }
                i += 1 + span;
                continue;
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
                        .is_some_and(|w| {
                            w.chars().count() == 1 && w.chars().all(char::is_alphabetic)
                        });

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
                out.push(if *tight {
                    Tok::tight(*lit)
                } else {
                    Tok::lit(*lit)
                });
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
///
/// That protection only holds *within a single pass*: `CaseTransforms` and the
/// dictionary mark a resolved identifier by turning it into a `Tok::Lit`, but
/// that marker cannot survive being rendered back to a plain string and
/// re-parsed. Formatting is required to be idempotent on its own output (history
/// replay depends on it — see `formatting_is_idempotent_on_already_clean_text`),
/// so a second pass over `"userName, then return"` must not capitalize `userName`
/// into `UserName` just because it is now a fresh `Tok::Word` again. See
/// `looks_like_an_identifier`.
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
                    if at_sentence_start && !looks_like_an_identifier(w) {
                        *w = capitalize(w);
                    }
                    at_sentence_start = false;
                }
            }
        }
        Doc { toks }
    }
}

/// Whether `w` is shaped like a program identifier rather than an ordinary
/// spoken word: lowercase first letter, with an uppercase letter somewhere
/// after it (`userName`, `useEffect`, `xmlHttpRequest`).
///
/// Ordinary English speech transcribed by Whisper never produces this shape —
/// a word is either all-lowercase, capitalized only on its first letter, or a
/// fully uppercase acronym. A lowercase-starting word with an internal capital
/// is, for this app's purposes, always a `camelCase` identifier that survived
/// from an earlier formatting pass (see `Capitalize`'s doc comment).
fn looks_like_an_identifier(w: &str) -> bool {
    let mut chars = w.chars();
    match chars.next() {
        Some(first) if first.is_lowercase() => chars.any(char::is_uppercase),
        _ => false,
    }
}

/// Whether `w` has an uppercase letter anywhere after its first character.
///
/// Used by `ProfilePolicy`'s force-lowercase policy for the same reason
/// `looks_like_an_identifier` exists: ordinary spoken English is never
/// uppercase in the middle of a word, so a word that is (`MAX_SIZE`,
/// `UserName`) is a constant or identifier that survived an earlier
/// formatting pass as plain text, and touching only its first character would
/// produce a broken, internally-inconsistent result rather than a correct one.
fn has_internal_uppercase(w: &str) -> bool {
    w.chars().skip(1).any(char::is_uppercase)
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
            //
            // Skip words with an uppercase letter anywhere *after* the first --
            // `SCREAMING_SNAKE_CASE` constants and `PascalCase` identifiers are
            // both a `Tok::Lit` and therefore untouched on the pass that builds
            // them, but a second pass over already-formatted text re-parses them
            // as a plain `Tok::Word`. Force-lowering just the first character of
            // `MAX_SIZE` does not produce a valid alternate casing, it produces
            // the broken, inconsistent `mAX_SIZE` -- so leave any such word
            // alone entirely rather than partially mangle it.
            if let Some(Tok::Word(w)) = toks.first_mut() {
                if !has_internal_uppercase(w) {
                    let mut cs = w.chars();
                    if let Some(c) = cs.next() {
                        *w = c.to_lowercase().collect::<String>() + cs.as_str();
                    }
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
    fn collapses_degenerate_character_runs() {
        // Real output observed from the user's first working session.
        let p = Profile::default();
        assert_eq!(
            run(&CollapseRepeats, &p, "?????????????????????????????Hello"),
            "???Hello"
        );
        assert_eq!(run(&CollapseRepeats, &p, "kkkkkkkkkkkkkkkkkkkk"), "kkk");
        assert_eq!(
            run(&CollapseRepeats, &p, "wait......................."),
            "wait..."
        );
    }

    #[test]
    fn collapses_repeated_word_loops() {
        let p = Profile::default();
        assert_eq!(
            run(&CollapseRepeats, &p, "hello Hello Hello Hello Hello there"),
            "hello Hello Hello there"
        );
        assert_eq!(
            run(&CollapseRepeats, &p, "good job good job good job good job"),
            "good job good job good job"
        );
    }

    #[test]
    fn leaves_legitimate_repetition_alone() {
        // The guard must not "fix" speech the user actually produced.
        let p = Profile::default();
        assert_eq!(run(&CollapseRepeats, &p, "no no no"), "no no no");
        assert_eq!(
            run(&CollapseRepeats, &p, "that is very very good"),
            "that is very very good"
        );
        assert_eq!(
            run(&CollapseRepeats, &p, "wait... really?"),
            "wait... really?"
        );
        assert_eq!(run(&CollapseRepeats, &p, "stop!!!"), "stop!!!");
        // Ordinary doubled letters inside words must survive untouched.
        assert_eq!(
            run(&CollapseRepeats, &p, "committee address bookkeeper"),
            "committee address bookkeeper"
        );
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
            run(
                &VoiceCommands,
                &p,
                "cube control get pods dash dash all namespaces"
            ),
            "cube control get pods --all namespaces"
        );
        assert_eq!(run(&VoiceCommands, &p, "well dash known"), "well-known");
        // A hyphenated long flag: one dash inside the name stays a hyphen.
        assert_eq!(
            run(
                &VoiceCommands,
                &p,
                "cube control get pods dash dash all dash namespaces"
            ),
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
    fn literally_escapes_a_whole_two_word_command_not_just_its_first_word() {
        // Regression: the escape used to protect only the token right after
        // "literally". "fat arrow" is a two-word COMMANDS entry (-> "=>"), but
        // bare "arrow" is *also* its own standalone entry (-> "->"), so escaping
        // only "fat" left "arrow" exposed to the very next loop iteration, which
        // reinterpreted it: "literally fat arrow" rendered as "fat ->" instead of
        // the literal words "fat arrow".
        let p = Profile::default();
        assert_eq!(run(&VoiceCommands, &p, "literally fat arrow"), "fat arrow");
        // Same failure shape for "new line" / "new paragraph": the second word
        // must not slip through to layout interpretation.
        assert_eq!(run(&VoiceCommands, &p, "literally new line"), "new line");
        assert_eq!(
            run(&VoiceCommands, &p, "literally new paragraph"),
            "new paragraph"
        );
        assert_eq!(
            run(&VoiceCommands, &p, "literally scratch that"),
            "scratch that"
        );
    }

    #[test]
    fn literally_escapes_a_whole_dash_run() {
        // Regression: a dash run is variable length, not one word. Escaping only
        // the first "dash" left the second free to be reinterpreted as a tight
        // hyphen, rendering "literally dash dash all" as "dash-all" instead of
        // the literal words "dash dash all".
        let p = Profile::default();
        assert_eq!(
            run(&VoiceCommands, &p, "literally dash dash all"),
            "dash dash all"
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
    fn capitalize_does_not_re_capitalize_an_identifier_on_a_second_pass() {
        // Regression: `Tok::Lit` protection (the test above) only holds within a
        // single pass. Once formatted output is rendered to a plain string and
        // re-parsed -- exactly what history replay and a live "preview this
        // phrase" feature both do -- a resolved identifier is just a `Tok::Word`
        // again, indistinguishable from an ordinary word by tokenization alone.
        // Feeding "userName, then return" straight into `Doc::parse` used to
        // capitalize it into "UserName, then return" the second time around.
        let p = Profile::default();
        assert_eq!(
            run(&Capitalize, &p, "userName, then return"),
            "userName, then return"
        );
        assert_eq!(
            run(&Capitalize, &p, "useEffect runs late"),
            "useEffect runs late"
        );
        // An ordinary lowercase word at sentence start must still capitalize.
        assert_eq!(run(&Capitalize, &p, "hello there"), "Hello there");
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
    fn terminal_profile_does_not_partially_mangle_a_preserved_case_identifier() {
        // Regression: force-lowercase used to touch the first character
        // unconditionally. On a first pass a `SCREAMING_SNAKE_CASE` constant or
        // `PascalCase` identifier from `CaseTransforms` is a `Tok::Lit` and is
        // never reached here, but re-parsing already-formatted text (history
        // replay, a live preview) turns it back into a plain `Tok::Word` --
        // and force-lowering just its first letter produced the broken
        // `mAX_SIZE` / `userName` (from `UserName`) instead of leaving either
        // one alone.
        let p = Profile::terminal();
        assert_eq!(run(&ProfilePolicy, &p, "MAX_SIZE = ten"), "MAX_SIZE = ten");
        assert_eq!(
            run(&ProfilePolicy, &p, "UserName is set"),
            "UserName is set"
        );
        // Ordinary sentence-initial capitalization from Whisper must still be
        // undone -- this is the entire point of the terminal profile.
        assert_eq!(run(&ProfilePolicy, &p, "Ls dash l"), "ls dash l");
    }

    #[test]
    fn prose_profile_adds_a_terminal_period_only_when_missing() {
        let p = Profile::prose();
        assert_eq!(run(&ProfilePolicy, &p, "hello there"), "hello there.");
        assert_eq!(run(&ProfilePolicy, &p, "hello there!"), "hello there!");
    }
}
