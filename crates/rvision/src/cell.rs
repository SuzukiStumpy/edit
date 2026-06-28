//! The screen cell: a grapheme cluster, its display width, and its style.
//!
//! See ADR 0006 — "one visible character" can be several Unicode scalars and can
//! occupy one or two terminal columns, so a cell is more than a `char`.

use crate::color::Style;
use core::fmt;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Repr {
    Single(char),
    Cluster(Box<str>),
}

/// A single grapheme cluster: what a reader perceives as one character. It may
/// be one Unicode scalar or several (a base plus combining marks, a ZWJ emoji
/// sequence, ...).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Grapheme(Repr);

impl Grapheme {
    /// Creates a grapheme from a single scalar (stored inline, no allocation).
    pub fn from_char(c: char) -> Self {
        Grapheme(Repr::Single(c))
    }

    /// Creates a grapheme from a string slice. A single scalar is stored inline;
    /// anything longer is boxed as a cluster.
    pub fn new(s: &str) -> Self {
        let mut chars = s.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Grapheme(Repr::Single(c)),
            _ => Grapheme(Repr::Cluster(s.into())),
        }
    }

    /// The display width in terminal columns: 0 for control/zero-width, 2 for
    /// wide characters (CJK, wide emoji), 1 for most.
    pub fn width(&self) -> u16 {
        let columns = match &self.0 {
            Repr::Single(c) => c.width().unwrap_or(0),
            Repr::Cluster(s) => s.width(),
        };
        columns as u16
    }
}

impl fmt::Display for Grapheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Repr::Single(c) => write!(f, "{c}"),
            Repr::Cluster(s) => write!(f, "{s}"),
        }
    }
}

/// One terminal cell: a grapheme, its cached display width, and its style.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Cell {
    grapheme: Grapheme,
    width: u16,
    style: Style,
}

impl Cell {
    /// Creates a cell from a grapheme and style; the width is computed once here.
    pub fn new(grapheme: Grapheme, style: Style) -> Self {
        let width = grapheme.width();
        Self {
            grapheme,
            width,
            style,
        }
    }

    /// Creates a cell from a single scalar.
    pub fn from_char(c: char, style: Style) -> Self {
        Self::new(Grapheme::from_char(c), style)
    }

    /// Creates a blank cell (a single space) with the given style.
    pub fn blank(style: Style) -> Self {
        Self::from_char(' ', style)
    }

    /// Creates a zero-width continuation cell: the placeholder that occupies the
    /// second column of a wide (two-column) cell. Renderers skip it.
    pub fn continuation(style: Style) -> Self {
        Self::new(Grapheme::new(""), style)
    }

    /// The cell's grapheme.
    pub fn grapheme(&self) -> &Grapheme {
        &self.grapheme
    }

    /// The cell's display width in columns.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// The cell's style.
    pub fn style(&self) -> Style {
        self.style
    }
}

impl Default for Cell {
    fn default() -> Self {
        Self::blank(Style::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Style;

    // Tracer bullet: a plain ASCII cell is width 1 and keeps its style.
    #[test]
    fn from_char_has_width_one_and_keeps_style() {
        let style = Style::new();
        let cell = Cell::from_char('A', style);
        assert_eq!(cell.width(), 1);
        assert_eq!(cell.style(), style);
        assert_eq!(cell.grapheme().to_string(), "A");
    }

    #[test]
    fn wide_scalar_is_width_two() {
        let cell = Cell::from_char('世', Style::new());
        assert_eq!(cell.width(), 2);
    }

    #[test]
    fn combining_cluster_is_one_grapheme_width_one() {
        // "e" + combining acute accent is one grapheme occupying one column.
        let g = Grapheme::new("e\u{0301}");
        assert_eq!(g.width(), 1);
        assert_eq!(g.to_string(), "e\u{0301}");
        assert_eq!(Cell::new(g, Style::new()).width(), 1);
    }

    #[test]
    fn combining_mark_alone_is_zero_width() {
        assert_eq!(Grapheme::from_char('\u{0301}').width(), 0);
    }

    #[test]
    fn continuation_is_zero_width_and_empty() {
        let c = Cell::continuation(Style::new());
        assert_eq!(c.width(), 0);
        assert_eq!(c.grapheme().to_string(), "");
    }

    #[test]
    fn default_cell_is_a_blank_space() {
        let cell = Cell::default();
        assert_eq!(cell.grapheme().to_string(), " ");
        assert_eq!(cell.width(), 1);
        assert_eq!(cell.style(), Style::new());
    }
}
