//! Word error rate: the accuracy number every speed change has to keep.

/// Lower-case words, with punctuation other than apostrophes treated as spaces.
#[must_use]
pub fn normalize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

/// Word-level edit distance: substitutions + insertions + deletions.
#[must_use]
pub fn errors(reference: &[String], hypothesis: &[String]) -> usize {
    let mut prev: Vec<usize> = (0..=hypothesis.len()).collect();
    for (i, r) in reference.iter().enumerate() {
        let mut cur = vec![i + 1; hypothesis.len() + 1];
        for (j, h) in hypothesis.iter().enumerate() {
            let substitute = prev[j] + usize::from(r != h);
            cur[j + 1] = substitute.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[hypothesis.len()]
}

/// Corpus WER in percent: total errors over total reference words.
///
/// Pooled rather than averaged per clip, so a two-word clip with one error
/// cannot outweigh a forty-word clip with none.
#[must_use]
pub fn wer(pairs: &[(String, String)]) -> f64 {
    let (mut errs, mut words) = (0usize, 0usize);
    for (reference, hypothesis) in pairs {
        let r = normalize(reference);
        errs += errors(&r, &normalize(hypothesis));
        words += r.len();
    }
    if words == 0 {
        0.0
    } else {
        100.0 * errs as f64 / words as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(r: &str, h: &str) -> f64 {
        wer(&[(r.to_owned(), h.to_owned())])
    }

    #[test]
    fn a_perfect_transcript_has_no_errors() {
        assert_eq!(w("the cat sat", "the cat sat"), 0.0);
    }

    #[test]
    fn substitutions_insertions_and_deletions_each_cost_one() {
        assert_eq!(
            errors(&normalize("the cat sat"), &normalize("the bat sat")),
            1
        );
        assert_eq!(
            errors(&normalize("the cat"), &normalize("the black cat")),
            1
        );
        assert_eq!(
            errors(&normalize("the black cat"), &normalize("the cat")),
            1
        );
        assert!((w("the cat sat", "the bat sat") - 100.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn case_and_punctuation_are_not_word_errors() {
        // LibriSpeech references are upper case with no punctuation; Parakeet
        // writes sentences. Comparing them raw would score formatting, not hearing.
        assert_eq!(w("HELLO WORLD", "Hello, world."), 0.0);
    }

    #[test]
    fn apostrophes_are_part_of_a_word() {
        assert_eq!(w("DON'T GO", "dont go"), 50.0);
    }
}
