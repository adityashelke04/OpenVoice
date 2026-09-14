//! Splitting a long transcript into paste-sized pieces.
//!
//! # Why a transcript is not pasted in one go
//!
//! Claude Code collapses any paste longer than 800 characters (or three lines)
//! into a `[Pasted text #N]` placeholder. The full text is still there and is
//! sent on submit, but the paragraph the user just spoke vanishes from the input
//! box — which reads as "the dictation didn't paste" and was reported exactly
//! that way. Measured in an Orca Claude Code pane on 2026-09-14: a 1081-character
//! transcript pasted whole showed only `[Pasted text #1]`; the same text pasted
//! as 697 + 384 characters 250 ms apart showed the whole paragraph inline, in
//! order, with the word boundary intact.
//!
//! Pieces break *after* whitespace, so the space stays at the end of the earlier
//! piece and no target can glue two words together.

/// Split `text` into consecutive pieces that concatenate back to exactly `text`.
///
/// Each piece holds at most `max_chars` characters (not bytes) and at most
/// `max_line_breaks` newlines. A piece ends just after the last whitespace that
/// fits; only a single run of non-whitespace longer than `max_chars` is cut
/// mid-run, because the alternative is overflowing the limit.
#[must_use]
pub fn paste_chunks(text: &str, max_chars: usize, max_line_breaks: usize) -> Vec<&str> {
    let max_chars = max_chars.max(1);
    let mut pieces = Vec::new();
    let mut rest = text;

    while !rest.is_empty() {
        let mut end = rest.len();
        let mut after_whitespace = None;
        let mut line_breaks = 0;

        for (chars, (i, c)) in rest.char_indices().enumerate() {
            if chars == max_chars {
                end = after_whitespace.unwrap_or(i);
                break;
            }
            if c.is_whitespace() {
                after_whitespace = Some(i + c.len_utf8());
            }
            if c == '\n' {
                line_breaks += 1;
                if line_breaks >= max_line_breaks.max(1) {
                    end = i + 1;
                    break;
                }
            }
        }

        pieces.push(&rest[..end]);
        rest = &rest[end..];
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARAGRAPH: &str = "I want you to review the website that is live currently on \
        Warsaw. Do a complete review of that. And do note that I want you to use the UIUX \
        Pro Max and the Impeccable skill and improve the website significantly. Do note \
        that this is a SaaS product and open source. So it should be popped out and it \
        should be shown to the people.";

    #[test]
    fn short_text_is_a_single_piece() {
        assert_eq!(paste_chunks("Hello world.", 600, 2), vec!["Hello world."]);
    }

    #[test]
    fn pieces_rejoin_to_the_exact_original() {
        let text = PARAGRAPH.repeat(5);
        let pieces = paste_chunks(&text, 120, 2);
        assert!(pieces.len() > 1, "long text must be split");
        assert_eq!(
            pieces.concat(),
            text,
            "not a single character may be lost or added"
        );
    }

    #[test]
    fn no_piece_exceeds_the_character_limit() {
        let text = PARAGRAPH.repeat(5);
        for piece in paste_chunks(&text, 120, 2) {
            assert!(
                piece.chars().count() <= 120,
                "{} chars: {piece:?}",
                piece.chars().count()
            );
        }
    }

    #[test]
    fn pieces_break_after_whitespace_so_no_word_is_cut() {
        let text = PARAGRAPH.repeat(5);
        let pieces = paste_chunks(&text, 120, 2);
        for piece in &pieces[..pieces.len() - 1] {
            assert!(
                piece.ends_with(char::is_whitespace),
                "piece cuts a word in half: {piece:?}"
            );
        }
    }

    #[test]
    fn a_word_longer_than_the_limit_is_split_rather_than_overflowing() {
        let text = "x".repeat(250);
        let pieces = paste_chunks(&text, 100, 2);
        assert_eq!(pieces.concat(), text);
        assert!(pieces.iter().all(|p| p.chars().count() <= 100));
    }

    #[test]
    fn limits_count_characters_not_bytes() {
        // 3-byte characters: a byte-based limit would split far too early, and a
        // naive byte slice would panic mid-character.
        let text = "é€ ".repeat(100); // 300 chars
        let pieces = paste_chunks(&text, 150, 2);
        assert_eq!(pieces.concat(), text);
        assert_eq!(pieces.len(), 2);
    }

    #[test]
    fn no_piece_carries_more_than_the_allowed_line_breaks() {
        let text = "one\ntwo\nthree\nfour\nfive\nsix\nseven";
        let pieces = paste_chunks(text, 600, 2);
        assert_eq!(pieces.concat(), text);
        for piece in &pieces {
            assert!(
                piece.matches('\n').count() <= 2,
                "too many lines: {piece:?}"
            );
        }
    }
}
