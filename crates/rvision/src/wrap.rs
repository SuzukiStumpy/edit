//! Word-wrapping for dialog and help prose.
//!
//! A pure helper that breaks paragraphs into lines fitting a maximum display
//! width, so callers pass prose rather than pre-broken lines (the ADR 0022
//! follow-up). Width is measured in display columns, the same unit the
//! [`Cell`](crate::cell)/[`Buffer`](crate::buffer) path uses (ADR 0006/0015).

use unicode_width::UnicodeWidthStr;

/// Wraps `text` to at most `width` display columns per line, breaking on ASCII
/// spaces. Returns the wrapped lines, without trailing newlines.
///
/// Existing `'\n'` are hard breaks and always survive (a blank line stays
/// blank). Within a hard line, words are greedily packed: a word joins the
/// current line if the joined result is `<= width` columns, otherwise it starts
/// the next one. Runs of spaces collapse to a single separator. A single word
/// wider than `width` takes its own line and is allowed to overflow rather than
/// split a grapheme cluster; `width == 0` therefore yields one word per line.
/// An empty string wraps to one empty line (`[""]`).
pub fn wrap(text: &str, width: u16) -> Vec<String> {
    let width = width as usize;
    let mut out = Vec::new();
    // Hard breaks first, so '\n' (and the blank spacer lines callers rely on,
    // ADR 0022) always survive intact.
    for hard_line in text.split('\n') {
        let mut current = String::new();
        // `split_whitespace` collapses runs of spaces into single separators.
        for word in hard_line.split_whitespace() {
            if current.is_empty() {
                current.push_str(word);
            } else if current.width() + 1 + word.width() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                // The word would overflow: flush the line and start anew. A word
                // wider than `width` simply becomes its own (overflowing) line.
                out.push(std::mem::take(&mut current));
                current.push_str(word);
            }
        }
        // Emit the line even when empty, so a blank hard line stays blank.
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(s: &str) -> u16 {
        s.width() as u16
    }

    #[test]
    fn short_text_passes_through_unchanged() {
        assert_eq!(wrap("a short line", 40), vec!["a short line"]);
    }

    #[test]
    fn empty_string_is_one_empty_line() {
        assert_eq!(wrap("", 40), vec![""]);
    }

    #[test]
    fn hard_breaks_and_blank_lines_survive() {
        assert_eq!(
            wrap("one\n\ntwo", 40),
            vec!["one".to_string(), String::new(), "two".to_string()]
        );
    }

    #[test]
    fn a_long_line_breaks_on_spaces_within_width() {
        let out = wrap("the quick brown fox jumps", 9);
        // Greedy: "the quick" (9) | "brown fox" (9) | "jumps".
        assert_eq!(out, vec!["the quick", "brown fox", "jumps"]);
        for line in &out {
            assert!(cols(line) <= 9, "{line:?} fits the width");
        }
    }

    #[test]
    fn runs_of_spaces_collapse_to_one_separator() {
        assert_eq!(wrap("a    b", 40), vec!["a b"]);
    }

    #[test]
    fn an_over_long_word_lands_alone_and_overflows() {
        let out = wrap("hi superlongword ok", 6);
        assert_eq!(out, vec!["hi", "superlongword", "ok"]);
    }

    #[test]
    fn wide_text_breaks_by_columns_not_chars() {
        // Each CJK ideograph is two columns: two of them already fill width 4,
        // so a third wraps even though it is only the third character.
        let out = wrap("世界 你好", 4);
        assert_eq!(out, vec!["世界", "你好"]);
    }

    #[test]
    fn every_line_fits_unless_it_is_one_over_long_word() {
        let text = "alpha beta gammagammagamma delta epsilon zeta eta theta";
        let width = 12;
        for line in wrap(text, width) {
            let single_word = !line.contains(' ');
            assert!(
                cols(&line) <= width || single_word,
                "{line:?} either fits or is a lone over-long word"
            );
        }
    }
}
