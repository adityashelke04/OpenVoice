//! The token document that rules operate on.
//!
//! Rules do not manipulate strings directly. They rewrite a token vector, and
//! rendering to text happens once at the end. This matters because spacing is
//! contextual — `foo ( bar )` is wrong, `foo(bar)` is right — and a rule that
//! inserts a `(` has no business also knowing how to space it. Centralizing that
//! knowledge in [`Doc::render`] keeps every rule small and independently testable.

/// One unit of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    /// A spoken word. Subject to further rewriting by later rules.
    Word(String),
    /// Literal text that is already final — punctuation, symbols, an identifier a
    /// rule has resolved. Later rules leave these alone. Spacing follows the
    /// [`TIGHT_LEFT`] / [`TIGHT_RIGHT`] tables.
    Lit(String),
    /// A literal that binds tightly to *both* neighbours, with no space on either
    /// side.
    ///
    /// This is what makes `dash dash all` render as `--all` rather than `- - all`,
    /// and `well dash known` as `well-known`. It also separates the two things a
    /// full stop can mean: "dot" is tight (`user.name`) while "period" ends a
    /// sentence (`done. Next`). Without the distinction one of those two is always
    /// wrong, and both are common in dictation.
    Tight(String),
    /// A line break. One for a new line, two consecutive for a paragraph.
    Break,
}

impl Tok {
    /// A word token, for terser rule code.
    pub fn word(s: impl Into<String>) -> Self {
        Self::Word(s.into())
    }

    /// A literal token.
    pub fn lit(s: impl Into<String>) -> Self {
        Self::Lit(s.into())
    }

    /// A literal that binds tightly to both neighbours.
    pub fn tight(s: impl Into<String>) -> Self {
        Self::Tight(s.into())
    }

    /// The token's text, whatever its kind.
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Word(s) | Self::Lit(s) | Self::Tight(s) => s,
            Self::Break => "\n",
        }
    }

    /// Lowercased word text, or `None` for non-words. The workhorse of matching.
    #[must_use]
    pub fn as_word_lower(&self) -> Option<String> {
        match self {
            Self::Word(s) => Some(s.to_lowercase()),
            _ => None,
        }
    }
}

/// A tokenized utterance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Doc {
    /// The tokens, in order.
    pub toks: Vec<Tok>,
}

/// Characters that never take a space before them.
const TIGHT_LEFT: &[char] = &[',', '.', ';', ':', '!', '?', ')', ']', '}', '%'];
/// Characters that never take a space after them.
///
/// `-` is here so a run of dashes introducing a command-line flag renders as
/// `--all` rather than `-- all`, while still taking a space *before* the run.
const TIGHT_RIGHT: &[char] = &['(', '[', '{', '$', '#', '@', '-'];

impl Doc {
    /// Split raw model output into word tokens.
    ///
    /// Trailing punctuation is peeled into its own [`Tok::Lit`] so that a rule
    /// matching the word `paren` is not defeated by the model having written
    /// `paren.` — a surprisingly common source of "why didn't my command work".
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        let mut toks = Vec::new();
        for chunk in raw.split_whitespace() {
            let core = chunk.trim_end_matches(|c: char| TIGHT_LEFT.contains(&c));
            let tail = &chunk[core.len()..];
            if !core.is_empty() {
                toks.push(Tok::Word(core.to_string()));
            }
            for c in tail.chars() {
                toks.push(Tok::Lit(c.to_string()));
            }
        }
        Self { toks }
    }

    /// Render to final text, applying contextual spacing.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        let mut pending_space = false;
        let mut suppress_next_space = false;

        for tok in &self.toks {
            match tok {
                Tok::Break => {
                    // A break swallows surrounding spaces; nobody wants "word \n".
                    while out.ends_with(' ') {
                        out.pop();
                    }
                    out.push('\n');
                    pending_space = false;
                    suppress_next_space = true;
                }
                Tok::Tight(s) => {
                    if s.is_empty() {
                        continue;
                    }
                    while out.ends_with(' ') {
                        out.pop();
                    }
                    out.push_str(s);
                    pending_space = true;
                    suppress_next_space = true;
                }
                Tok::Word(s) | Tok::Lit(s) => {
                    if s.is_empty() {
                        continue;
                    }
                    let first = s.chars().next().unwrap_or(' ');
                    let tight_left = TIGHT_LEFT.contains(&first);
                    if pending_space && !tight_left && !suppress_next_space {
                        out.push(' ');
                    }
                    out.push_str(s);
                    let last = s.chars().last().unwrap_or(' ');
                    suppress_next_space = TIGHT_RIGHT.contains(&last);
                    pending_space = true;
                }
            }
        }
        out.trim_end_matches(' ').to_string()
    }

    /// Number of tokens.
    #[must_use]
    pub fn len(&self) -> usize {
        self.toks.len()
    }

    /// Whether the document has no tokens.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.toks.is_empty()
    }

    /// Lowercased words starting at `i`, up to `n` of them.
    ///
    /// Returns `None` if fewer than `n` word tokens are available, which lets
    /// multi-word matchers bail out cheaply at the end of a document.
    #[must_use]
    pub fn window(&self, i: usize, n: usize) -> Option<Vec<String>> {
        let mut out = Vec::with_capacity(n);
        for tok in self.toks.get(i..)?.iter().take(n) {
            out.push(tok.as_word_lower()?);
        }
        (out.len() == n).then_some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn peels_trailing_punctuation_into_its_own_token() {
        let d = Doc::parse("open paren.");
        assert_eq!(
            d.toks,
            vec![Tok::word("open"), Tok::word("paren"), Tok::lit(".")]
        );
    }

    #[test]
    fn renders_punctuation_tight_to_the_left() {
        let d = Doc {
            toks: vec![Tok::word("hello"), Tok::lit(","), Tok::word("world")],
        };
        assert_eq!(d.render(), "hello, world");
    }

    #[test]
    fn renders_brackets_tight_on_the_inside() {
        let d = Doc {
            toks: vec![
                Tok::word("foo"),
                Tok::lit("("),
                Tok::word("bar"),
                Tok::lit(")"),
            ],
        };
        assert_eq!(d.render(), "foo (bar)");
    }

    #[test]
    fn breaks_swallow_surrounding_spaces() {
        let d = Doc {
            toks: vec![Tok::word("one"), Tok::Break, Tok::word("two")],
        };
        assert_eq!(d.render(), "one\ntwo");
    }

    #[test]
    fn tight_tokens_bind_to_both_neighbours() {
        // "well dash known" -> "well-known", not "well - known".
        let d = Doc {
            toks: vec![Tok::word("well"), Tok::tight("-"), Tok::word("known")],
        };
        assert_eq!(d.render(), "well-known");
    }

    #[test]
    fn adjacent_tight_tokens_join() {
        // "dash dash all" -> "--all". Rendering these apart produced
        // `kubectl get pods - - all namespaces`, which is not a runnable command.
        let d = Doc {
            toks: vec![Tok::tight("-"), Tok::tight("-"), Tok::word("all")],
        };
        assert_eq!(d.render(), "--all");
    }

    #[test]
    fn tight_and_sentence_punctuation_differ() {
        let dotted = Doc {
            toks: vec![Tok::word("user"), Tok::tight("."), Tok::word("name")],
        };
        assert_eq!(dotted.render(), "user.name");

        let sentence = Doc {
            toks: vec![Tok::word("done"), Tok::lit("."), Tok::word("Next")],
        };
        assert_eq!(sentence.render(), "done. Next");
    }

    #[test]
    fn window_stops_at_non_words() {
        let d = Doc {
            toks: vec![Tok::word("a"), Tok::lit("."), Tok::word("b")],
        };
        assert_eq!(d.window(0, 1), Some(vec!["a".to_string()]));
        assert_eq!(d.window(0, 2), None, "must not span a literal");
        assert_eq!(d.window(0, 9), None, "must not run past the end");
    }
}
