//! The editor's in-memory document: lines of text, mutated only through
//! reversible [`Edit`]s.
//!
//! This is pure logic — no screen, no styling, no viewport (see ADR 0008 for the
//! line-array representation and ADR 0011 for reversible editing). Positions are
//! `(line, grapheme-column)`; content is plain UTF-8 `String`s. Rendering a
//! document is the editor view's job in a later phase.

use unicode_segmentation::UnicodeSegmentation;

/// A document position: a 0-based `line` and a 0-based grapheme `column` within
/// that line. `column == line_graphemes(line)` is the (valid) end-of-line spot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Position {
    /// Line index, 0-based from the top of the document.
    pub line: usize,
    /// Grapheme column, 0-based from the start of the line.
    pub column: usize,
}

impl Position {
    /// Creates a position at `(line, column)`.
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

/// A reversible document mutation (ADR 0011).
///
/// `Insert` and `Delete` are exact inverses: each carries the full `text`
/// involved, so the affected span is derivable from `at` and `text` alone — no
/// buffer read is needed to invert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Insert `text` so that its first grapheme lands at `at`.
    Insert {
        /// Where the inserted text begins.
        at: Position,
        /// The text to insert (may contain `'\n'`).
        text: String,
    },
    /// Remove the span that starts at `at` and has the shape of `text`.
    Delete {
        /// Where the removed span begins.
        at: Position,
        /// The text being removed (its newline/grapheme shape defines the span).
        text: String,
    },
}

impl Edit {
    /// An insertion of `text` whose first grapheme lands at `at`.
    pub fn insert(at: Position, text: impl Into<String>) -> Self {
        Edit::Insert {
            at,
            text: text.into(),
        }
    }

    /// A deletion of the span starting at `at` with the shape of `text`.
    pub fn delete(at: Position, text: impl Into<String>) -> Self {
        Edit::Delete {
            at,
            text: text.into(),
        }
    }

    /// The inverse edit: insertion and deletion swap, `at` and `text` unchanged.
    /// Applying an edit then its inverse restores the buffer (ADR 0011).
    pub fn invert(&self) -> Edit {
        match self {
            Edit::Insert { at, text } => Edit::Delete {
                at: *at,
                text: text.clone(),
            },
            Edit::Delete { at, text } => Edit::Insert {
                at: *at,
                text: text.clone(),
            },
        }
    }
}

/// The byte offset of grapheme `column` within `line`; clamps to `line.len()`
/// for the end-of-line column (or any column past the end).
fn byte_of_column(line: &str, column: usize) -> usize {
    line.grapheme_indices(true)
        .nth(column)
        .map_or(line.len(), |(i, _)| i)
}

/// The position reached by walking `text` forward from `at` — i.e. where an
/// insertion of `text` ends, and the span a same-`text` deletion covers. The
/// editor uses it to land the caret at the far end of a paste.
pub(crate) fn position_after(at: Position, text: &str) -> Position {
    match text.rsplit_once('\n') {
        None => Position::new(at.line, at.column + text.graphemes(true).count()),
        Some((before, last)) => Position::new(
            at.line + before.matches('\n').count() + 1,
            last.graphemes(true).count(),
        ),
    }
}

/// The document representation seam (ADR 0008): the editor talks to text through
/// this trait, so the line array here can later be swapped for a gap buffer or
/// rope without touching the editor.
pub trait TextBuffer {
    /// The number of lines (always at least 1).
    fn line_count(&self) -> usize;

    /// The text of line `index`, or `None` if out of range.
    fn line(&self, index: usize) -> Option<&str>;

    /// Applies an [`Edit`], mutating the document in place.
    fn apply(&mut self, edit: &Edit);

    /// The number of graphemes on line `index` (0 if out of range).
    fn line_graphemes(&self, index: usize) -> usize {
        self.line(index).map_or(0, |l| l.graphemes(true).count())
    }

    /// The position one grapheme after `pos`, stepping to the start of the next
    /// line at a line end, or `None` at the document end.
    fn grapheme_after(&self, pos: Position) -> Option<Position> {
        if pos.line >= self.line_count() {
            return None;
        }
        if pos.column < self.line_graphemes(pos.line) {
            Some(Position::new(pos.line, pos.column + 1))
        } else if pos.line + 1 < self.line_count() {
            Some(Position::new(pos.line + 1, 0))
        } else {
            None
        }
    }

    /// The position one grapheme before `pos`, stepping to the end of the
    /// previous line at a line start, or `None` at the document start.
    fn grapheme_before(&self, pos: Position) -> Option<Position> {
        if pos.column > 0 {
            Some(Position::new(pos.line, pos.column - 1))
        } else if pos.line > 0 {
            Some(Position::new(
                pos.line - 1,
                self.line_graphemes(pos.line - 1),
            ))
        } else {
            None
        }
    }

    /// The document text from `start` up to (not including) `end`, with line
    /// breaks as `'\n'` — the exact shape an [`Edit::delete`] at `start` would
    /// remove. `start` must not come after `end`; a column past a line's end
    /// clamps to it. Used for selection copy/replace.
    fn slice(&self, start: Position, end: Position) -> String {
        if start.line == end.line {
            let line = self.line(start.line).unwrap_or("");
            let a = byte_of_column(line, start.column);
            let b = byte_of_column(line, end.column);
            return line[a.min(b)..a.max(b)].to_string();
        }
        let first = self.line(start.line).unwrap_or("");
        let mut out = first[byte_of_column(first, start.column)..].to_string();
        for index in (start.line + 1)..end.line {
            out.push('\n');
            out.push_str(self.line(index).unwrap_or(""));
        }
        out.push('\n');
        let last = self.line(end.line).unwrap_or("");
        out.push_str(&last[..byte_of_column(last, end.column)]);
        out
    }

    /// The whole document as a single string, lines joined by `'\n'`.
    fn to_text(&self) -> String {
        (0..self.line_count())
            .filter_map(|i| self.line(i))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Line-array document: one UTF-8 `String` per line (ADR 0008).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LineArray {
    lines: Vec<String>,
}

impl LineArray {
    /// Creates an empty document: a single empty line.
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
        }
    }
}

impl From<&str> for LineArray {
    fn from(text: &str) -> Self {
        Self {
            lines: text.split('\n').map(String::from).collect(),
        }
    }
}

impl TextBuffer for LineArray {
    fn line_count(&self) -> usize {
        self.lines.len()
    }

    fn line(&self, index: usize) -> Option<&str> {
        self.lines.get(index).map(String::as_str)
    }

    fn apply(&mut self, edit: &Edit) {
        match edit {
            Edit::Insert { at, text } => {
                let line = &self.lines[at.line];
                let split = byte_of_column(line, at.column);
                let head = line[..split].to_string();
                let tail = line[split..].to_string();
                match text.split_once('\n') {
                    None => self.lines[at.line] = format!("{head}{text}{tail}"),
                    Some((first, rest)) => {
                        let mut replacement = vec![format!("{head}{first}")];
                        let inner: Vec<&str> = rest.split('\n').collect();
                        let (last, middles) = inner.split_last().expect("split has >=1 part");
                        replacement.extend(middles.iter().map(|m| (*m).to_string()));
                        replacement.push(format!("{last}{tail}"));
                        self.lines.splice(at.line..=at.line, replacement);
                    }
                }
            }
            Edit::Delete { at, text } => {
                let end = position_after(*at, text);
                let start = &self.lines[at.line];
                let head = start[..byte_of_column(start, at.column)].to_string();
                let end_line = &self.lines[end.line];
                let tail = end_line[byte_of_column(end_line, end.column)..].to_string();
                self.lines
                    .splice(at.line..=end.line, [format!("{head}{tail}")]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tracer bullet: parse multi-line text, read it back faithfully.
    #[test]
    fn from_str_round_trips_lines() {
        let doc = LineArray::from("alpha\nbeta\ngamma");
        assert_eq!(doc.line_count(), 3);
        assert_eq!(doc.line(0), Some("alpha"));
        assert_eq!(doc.line(2), Some("gamma"));
        assert_eq!(doc.line(3), None);
        assert_eq!(doc.to_text(), "alpha\nbeta\ngamma");
    }

    #[test]
    fn empty_document_is_one_blank_line() {
        assert_eq!(LineArray::new().to_text(), "");
        assert_eq!(LineArray::new().line_count(), 1);
        // A trailing newline yields a trailing empty line (file-ends-in-EOL).
        assert_eq!(LineArray::from("x\n").line_count(), 2);
    }

    #[test]
    fn invert_swaps_variant_keeping_at_and_text() {
        let at = Position::new(1, 2);
        let ins = Edit::insert(at, "hi");
        let del = Edit::delete(at, "hi");
        assert_eq!(ins.invert(), del);
        assert_eq!(del.invert(), ins);
        assert_eq!(ins.invert().invert(), ins);
    }

    #[test]
    fn insert_within_a_line() {
        let mut doc = LineArray::from("Heo");
        doc.apply(&Edit::insert(Position::new(0, 2), "ll"));
        assert_eq!(doc.to_text(), "Hello");
    }

    #[test]
    fn insert_with_newline_splits_line() {
        let mut doc = LineArray::from("abcd");
        doc.apply(&Edit::insert(Position::new(0, 2), "X\nY"));
        assert_eq!(doc.to_text(), "abX\nYcd");
        assert_eq!(doc.line_count(), 2);
    }

    #[test]
    fn delete_within_a_line() {
        let mut doc = LineArray::from("Hello");
        doc.apply(&Edit::delete(Position::new(0, 2), "ll"));
        assert_eq!(doc.to_text(), "Heo");
    }

    #[test]
    fn delete_spanning_lines_joins() {
        let mut doc = LineArray::from("abX\nYcd");
        doc.apply(&Edit::delete(Position::new(0, 2), "X\nY"));
        assert_eq!(doc.to_text(), "abcd");
        assert_eq!(doc.line_count(), 1);
    }

    #[test]
    fn slice_within_and_across_lines() {
        let doc = LineArray::from("alpha\nbeta\ngamma");
        // Within one line.
        assert_eq!(doc.slice(Position::new(0, 1), Position::new(0, 4)), "lph");
        // Across lines: tail of first, whole middle, head of last.
        assert_eq!(
            doc.slice(Position::new(0, 3), Position::new(2, 2)),
            "ha\nbeta\nga"
        );
        // A slice is exactly what a Delete of that text would remove.
        let mut work = doc.clone();
        let span = doc.slice(Position::new(0, 3), Position::new(2, 2));
        work.apply(&Edit::delete(Position::new(0, 3), span));
        assert_eq!(work.to_text(), "alpmma");
    }

    #[test]
    fn line_graphemes_counts_clusters_not_bytes() {
        // wide char + combining sequence: 2 graphemes, many bytes.
        let doc = LineArray::from("界e\u{0301}");
        assert_eq!(doc.line_graphemes(0), 2);
        assert_eq!(doc.line_graphemes(9), 0); // out of range
    }

    #[test]
    fn grapheme_after_steps_and_crosses_lines() {
        let doc = LineArray::from("ab\ncd");
        assert_eq!(
            doc.grapheme_after(Position::new(0, 0)),
            Some(Position::new(0, 1))
        );
        // end of line 0 -> start of line 1
        assert_eq!(
            doc.grapheme_after(Position::new(0, 2)),
            Some(Position::new(1, 0))
        );
        // end of last line -> document end
        assert_eq!(doc.grapheme_after(Position::new(1, 2)), None);
    }

    #[test]
    fn grapheme_before_steps_and_crosses_lines() {
        let doc = LineArray::from("ab\ncd");
        assert_eq!(
            doc.grapheme_before(Position::new(1, 1)),
            Some(Position::new(1, 0))
        );
        // start of line 1 -> end of line 0
        assert_eq!(
            doc.grapheme_before(Position::new(1, 0)),
            Some(Position::new(0, 2))
        );
        // start of document -> None
        assert_eq!(doc.grapheme_before(Position::new(0, 0)), None);
    }

    // Property (hand-rolled; no proptest — crate budget): random edits applied
    // then inverted must restore the buffer exactly. Insert is the source of
    // truth; each Delete is generated as the inverse of a known Insert so its
    // captured `text` is guaranteed correct.
    #[test]
    fn apply_then_invert_restores_buffer() {
        let mut rng = Lcg::new(0x1234_5678);
        for _ in 0..2000 {
            let before = LineArray::from(rng.text().as_str());
            let at = rng.position(&before);
            let edit = if rng.next() & 1 == 0 {
                Edit::insert(at, rng.text())
            } else {
                // A valid Delete: insert random text, then its inverse removes it.
                let seed = Edit::insert(at, rng.text());
                let mut seeded = before.clone();
                seeded.apply(&seed);
                // Round-trip from the *seeded* buffer using the delete and its inverse.
                let delete = seed.invert();
                let mut work = seeded.clone();
                work.apply(&delete);
                work.apply(&delete.invert());
                assert_eq!(work, seeded, "delete round-trip failed for {delete:?}");
                continue;
            };
            let mut work = before.clone();
            work.apply(&edit);
            work.apply(&edit.invert());
            assert_eq!(work, before, "insert round-trip failed for {edit:?}");
        }
    }

    /// Tiny deterministic PRNG (xorshift-ish LCG) for the property test — keeps
    /// us inside the crate budget while giving reproducible randomised coverage.
    struct Lcg(u64);
    impl Lcg {
        fn new(seed: u64) -> Self {
            Self(seed)
        }
        fn next(&mut self) -> u64 {
            // SplitMix64.
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
        /// Random text over a small alphabet incl. `'\n'`, a wide char and a
        /// combining sequence, length 0..6.
        fn text(&mut self) -> String {
            const ALPHABET: [&str; 6] = ["a", "b", "\n", "界", "e\u{0301}", " "];
            let len = self.below(6);
            (0..len)
                .map(|_| ALPHABET[self.below(ALPHABET.len())])
                .collect()
        }
        /// A valid position somewhere inside `doc`.
        fn position(&mut self, doc: &LineArray) -> Position {
            let line = self.below(doc.line_count());
            let cols = doc.line_graphemes(line) + 1; // 0..=len inclusive
            Position::new(line, self.below(cols))
        }
    }
}
