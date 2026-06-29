//! Buffer text search: find a needle within the document, line by line, with
//! case-insensitive and whole-word options and optional wrap-around.
//!
//! Pure logic over any [`TextBuffer`] (it never mutates), so it is unit-tested
//! against a [`LineArray`](crate::text::LineArray) without an editor. Search is
//! line-oriented (the needle holds no newline), matching the spirit of MS-DOS
//! `EDIT`; positions are `(line, grapheme-column)` so a [`Match`] drops straight
//! into the editor's selection (see `docs/specs/search.md`).

use crate::text::{Position, TextBuffer};
use unicode_segmentation::UnicodeSegmentation;

/// What to search for and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// The text to find (no newline — search is within a line).
    pub needle: String,
    /// Match case exactly when `true`; fold case when `false`.
    pub case_sensitive: bool,
    /// Require non-word boundaries on both sides of the match when `true`.
    pub whole_word: bool,
}

/// A located match: the half-open span `[start, end)` on a single line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    /// First grapheme of the match.
    pub start: Position,
    /// One past the last grapheme (same line as `start`).
    pub end: Position,
}

impl Query {
    /// A case-insensitive, non-whole-word query for `needle`.
    pub fn new(needle: &str) -> Self {
        Self {
            needle: needle.to_string(),
            case_sensitive: false,
            whole_word: false,
        }
    }

    /// The first match found scanning from `from`, going forward (or `backward`),
    /// optionally wrapping past the document end/start back to `from`. Returns
    /// `None` for an empty needle or no match.
    pub fn find(
        &self,
        buf: &impl TextBuffer,
        from: Position,
        backward: bool,
        wrap: bool,
    ) -> Option<Match> {
        let n = buf.line_count();
        let qlen = self.needle.graphemes(true).count();
        if qlen == 0 || n == 0 {
            return None;
        }
        let at = |line: usize, col: usize| Match {
            start: Position::new(line, col),
            end: Position::new(line, col + qlen),
        };

        for (line, lo, hi) in self.windows(from, backward, wrap, n) {
            let cols = self.line_matches(buf.line(line).unwrap_or(""));
            let hit = if backward {
                cols.iter().rev().find(|&&c| c >= lo && c < hi)
            } else {
                cols.iter().find(|&&c| c >= lo && c < hi)
            };
            if let Some(&c) = hit {
                return Some(at(line, c));
            }
        }
        None
    }

    /// The ordered `(line, lo, hi)` column windows to scan: the start line is
    /// bounded by `from.column`; wrapping appends the far side and, last, the
    /// earlier part of the start line.
    fn windows(
        &self,
        from: Position,
        backward: bool,
        wrap: bool,
        n: usize,
    ) -> Vec<(usize, usize, usize)> {
        let mut w = Vec::new();
        if backward {
            w.push((from.line, 0, from.column));
            w.extend((0..from.line).rev().map(|l| (l, 0, usize::MAX)));
            if wrap {
                w.extend((from.line + 1..n).rev().map(|l| (l, 0, usize::MAX)));
                w.push((from.line, from.column, usize::MAX));
            }
        } else {
            w.push((from.line, from.column, usize::MAX));
            w.extend((from.line + 1..n).map(|l| (l, 0, usize::MAX)));
            if wrap {
                w.extend((0..from.line).map(|l| (l, 0, usize::MAX)));
                w.push((from.line, 0, from.column));
            }
        }
        w
    }

    /// The grapheme-column starts of every match of the needle in `line`.
    fn line_matches(&self, line: &str) -> Vec<usize> {
        let gl: Vec<&str> = line.graphemes(true).collect();
        let gq: Vec<&str> = self.needle.graphemes(true).collect();
        if gq.is_empty() || gq.len() > gl.len() {
            return Vec::new();
        }
        (0..=gl.len() - gq.len())
            .filter(|&s| self.matches_at(&gl, &gq, s))
            .collect()
    }

    /// Whether the needle matches the line graphemes starting at column `at`,
    /// honouring case folding and the whole-word boundary rule.
    fn matches_at(&self, gl: &[&str], gq: &[&str], at: usize) -> bool {
        if !gq
            .iter()
            .enumerate()
            .all(|(i, q)| self.eq_grapheme(gl[at + i], q))
        {
            return false;
        }
        if self.whole_word {
            let before_ok = at == 0 || !is_word(gl[at - 1]);
            let after = at + gq.len();
            let after_ok = after >= gl.len() || !is_word(gl[after]);
            return before_ok && after_ok;
        }
        true
    }

    /// Grapheme equality under the case-sensitivity setting.
    fn eq_grapheme(&self, a: &str, b: &str) -> bool {
        if self.case_sensitive {
            a == b
        } else {
            a.to_lowercase() == b.to_lowercase()
        }
    }
}

/// Whether `g`'s first char is a word character (alphanumeric or `_`).
fn is_word(g: &str) -> bool {
    g.chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::LineArray;

    fn buf(text: &str) -> LineArray {
        LineArray::from(text)
    }

    fn pos(line: usize, col: usize) -> Position {
        Position::new(line, col)
    }

    fn found(m: Option<Match>) -> (Position, Position) {
        let m = m.expect("a match");
        (m.start, m.end)
    }

    #[test]
    fn finds_a_match_within_a_line() {
        let b = buf("the quick brown fox");
        let q = Query::new("quick");
        assert_eq!(
            found(q.find(&b, pos(0, 0), false, false)),
            (pos(0, 4), pos(0, 9))
        );
    }

    #[test]
    fn case_insensitive_by_default_sensitive_on_request() {
        let b = buf("Hello HELLO hello");
        let mut q = Query::new("hello");
        // Insensitive: the first occurrence wins.
        assert_eq!(found(q.find(&b, pos(0, 0), false, false)).0, pos(0, 0));
        // Sensitive: only the exact-case "hello" at column 12.
        q.case_sensitive = true;
        assert_eq!(found(q.find(&b, pos(0, 0), false, false)).0, pos(0, 12));
    }

    #[test]
    fn whole_word_rejects_substrings() {
        let b = buf("a cat category catalog");
        let mut q = Query::new("cat");
        q.whole_word = true;
        // Only the standalone "cat" at column 2 matches.
        assert_eq!(found(q.find(&b, pos(0, 0), false, false)).0, pos(0, 2));
        // From just past it, with no wrap there is no other whole "cat".
        assert!(q.find(&b, pos(0, 5), false, false).is_none());
    }

    #[test]
    fn forward_search_crosses_lines() {
        let b = buf("alpha\nbeta\ngamma beta");
        let q = Query::new("beta");
        // From the start of line 1 finds line 1's "beta"...
        assert_eq!(found(q.find(&b, pos(1, 0), false, false)).0, pos(1, 0));
        // ...and from just after it, the next is on line 2.
        assert_eq!(found(q.find(&b, pos(1, 1), false, false)).0, pos(2, 6));
    }

    #[test]
    fn forward_wraps_around_to_an_earlier_match() {
        let b = buf("needle here\nand there");
        let q = Query::new("needle");
        // Starting past the only match: no match without wrap, found with wrap.
        assert!(q.find(&b, pos(0, 1), false, false).is_none());
        assert_eq!(found(q.find(&b, pos(0, 1), false, true)).0, pos(0, 0));
    }

    #[test]
    fn backward_search_finds_the_previous_match() {
        let b = buf("foo bar foo bar foo");
        let q = Query::new("foo");
        // Searching backward from column 12 lands on the "foo" at column 8.
        assert_eq!(found(q.find(&b, pos(0, 12), true, false)).0, pos(0, 8));
        // Backward from the very start: nothing without wrap; the last with wrap.
        assert!(q.find(&b, pos(0, 0), true, false).is_none());
        assert_eq!(found(q.find(&b, pos(0, 0), true, true)).0, pos(0, 16));
    }

    #[test]
    fn an_empty_needle_or_no_match_yields_none() {
        let b = buf("anything");
        assert!(Query::new("").find(&b, pos(0, 0), false, true).is_none());
        assert!(Query::new("zzz").find(&b, pos(0, 0), false, true).is_none());
    }

    #[test]
    fn matches_count_graphemes_not_bytes() {
        // "世" is one grapheme; the match after it starts at column 1, not 3.
        let b = buf("世xy");
        let q = Query::new("xy");
        assert_eq!(
            found(q.find(&b, pos(0, 0), false, false)),
            (pos(0, 1), pos(0, 3))
        );
    }
}
