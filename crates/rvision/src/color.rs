//! Colours and text styling.
//!
//! Truecolour-ready from the start (ADR 0005): a [`Color`] may be the terminal
//! default, one of the 16 named [`Color16`] palette entries, or an arbitrary
//! RGB triple. The named colours store their canonical CGA values, so the look
//! is identical whether a backend ultimately emits 16-colour or truecolour
//! escape sequences.

use core::ops::BitOr;

/// The 16 colours of the classic CGA/EGA text palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color16 {
    /// Black.
    Black,
    /// Blue.
    Blue,
    /// Green.
    Green,
    /// Cyan.
    Cyan,
    /// Red.
    Red,
    /// Magenta.
    Magenta,
    /// Brown (dark yellow).
    Brown,
    /// Light gray.
    LightGray,
    /// Dark gray (bright black).
    DarkGray,
    /// Light blue.
    LightBlue,
    /// Light green.
    LightGreen,
    /// Light cyan.
    LightCyan,
    /// Light red.
    LightRed,
    /// Light magenta.
    LightMagenta,
    /// Yellow (bright).
    Yellow,
    /// White (bright).
    White,
}

impl Color16 {
    /// Returns the colour's canonical CGA RGB value.
    pub const fn to_rgb(self) -> (u8, u8, u8) {
        match self {
            Color16::Black => (0, 0, 0),
            Color16::Blue => (0, 0, 170),
            Color16::Green => (0, 170, 0),
            Color16::Cyan => (0, 170, 170),
            Color16::Red => (170, 0, 0),
            Color16::Magenta => (170, 0, 170),
            Color16::Brown => (170, 85, 0),
            Color16::LightGray => (170, 170, 170),
            Color16::DarkGray => (85, 85, 85),
            Color16::LightBlue => (85, 85, 255),
            Color16::LightGreen => (85, 255, 85),
            Color16::LightCyan => (85, 255, 255),
            Color16::LightRed => (255, 85, 85),
            Color16::LightMagenta => (255, 85, 255),
            Color16::Yellow => (255, 255, 85),
            Color16::White => (255, 255, 255),
        }
    }
}

/// A colour: the terminal default, one of the 16 named palette entries, or an
/// arbitrary RGB triple (ADR 0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Color {
    /// The terminal's own default colour (no fixed RGB).
    #[default]
    Default,
    /// One of the 16 named CGA palette colours.
    Named(Color16),
    /// An arbitrary 24-bit colour.
    Rgb(u8, u8, u8),
}

impl Color {
    /// Resolves the colour to a concrete RGB triple, or `None` for
    /// [`Color::Default`] (which has no fixed value — it defers to the terminal).
    pub const fn resolve_rgb(self) -> Option<(u8, u8, u8)> {
        match self {
            Color::Default => None,
            Color::Named(c) => Some(c.to_rgb()),
            Color::Rgb(r, g, b) => Some((r, g, b)),
        }
    }
}

/// A set of text rendering attributes, stored as a bitset.
///
/// Combine with `|` (or [`Attributes::union`]); query with
/// [`Attributes::contains`], which is true only when *all* of the queried bits
/// are present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Attributes(u8);

impl Attributes {
    /// No attributes.
    pub const NONE: Self = Self(0);
    /// Bold / bright.
    pub const BOLD: Self = Self(1 << 0);
    /// Dim / faint.
    pub const DIM: Self = Self(1 << 1);
    /// Italic.
    pub const ITALIC: Self = Self(1 << 2);
    /// Underline.
    pub const UNDERLINE: Self = Self(1 << 3);
    /// Reverse video (swap fg/bg).
    pub const REVERSE: Self = Self(1 << 4);
    /// Blink.
    pub const BLINK: Self = Self(1 << 5);

    /// Returns the empty attribute set.
    pub const fn empty() -> Self {
        Self::NONE
    }

    /// Returns whether no attributes are set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns whether every bit in `other` is also set in `self`.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns the union of two attribute sets.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl BitOr for Attributes {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

/// The full rendering style of a cell: foreground, background, and attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Style {
    /// Foreground (text) colour.
    pub fg: Color,
    /// Background colour.
    pub bg: Color,
    /// Text attributes.
    pub attrs: Attributes,
}

impl Style {
    /// A blank style: default foreground and background, no attributes.
    pub const fn new() -> Self {
        Self {
            fg: Color::Default,
            bg: Color::Default,
            attrs: Attributes::NONE,
        }
    }

    /// Returns the style with its foreground colour replaced.
    pub const fn fg(mut self, fg: Color) -> Self {
        self.fg = fg;
        self
    }

    /// Returns the style with its background colour replaced.
    pub const fn bg(mut self, bg: Color) -> Self {
        self.bg = bg;
        self
    }

    /// Returns the style with its attributes replaced.
    pub const fn attrs(mut self, attrs: Attributes) -> Self {
        self.attrs = attrs;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tracer bullet: the named palette resolves to canonical CGA RGB values.
    #[test]
    fn color16_maps_to_canonical_cga_rgb() {
        assert_eq!(Color16::Black.to_rgb(), (0, 0, 0));
        assert_eq!(Color16::Blue.to_rgb(), (0, 0, 170));
        assert_eq!(Color16::LightGray.to_rgb(), (170, 170, 170));
        assert_eq!(Color16::Yellow.to_rgb(), (255, 255, 85));
        assert_eq!(Color16::White.to_rgb(), (255, 255, 255));
    }

    #[test]
    fn color_resolves_to_rgb() {
        assert_eq!(Color::Named(Color16::Red).resolve_rgb(), Some((170, 0, 0)));
        assert_eq!(Color::Rgb(12, 34, 56).resolve_rgb(), Some((12, 34, 56)));
        assert_eq!(Color::Default.resolve_rgb(), None);
    }

    #[test]
    fn attributes_combine_and_query() {
        assert!(Attributes::NONE.is_empty());
        assert!(!Attributes::NONE.contains(Attributes::BOLD));

        let combo = Attributes::BOLD | Attributes::UNDERLINE;
        assert!(combo.contains(Attributes::BOLD));
        assert!(combo.contains(Attributes::UNDERLINE));
        assert!(!combo.contains(Attributes::REVERSE));

        // `contains` means "all of": a multi-bit query matches only if every bit
        // is present.
        assert!(combo.contains(Attributes::BOLD | Attributes::UNDERLINE));
        assert!(!combo.contains(Attributes::BOLD | Attributes::REVERSE));

        // `union` is equivalent to the `|` operator.
        assert_eq!(Attributes::BOLD.union(Attributes::UNDERLINE), combo);
    }

    #[test]
    fn style_default_is_blank_and_builders_chain() {
        let blank = Style::new();
        assert_eq!(blank.fg, Color::Default);
        assert_eq!(blank.bg, Color::Default);
        assert!(blank.attrs.is_empty());
        assert_eq!(Style::default(), blank);

        let s = Style::new()
            .fg(Color::Named(Color16::Yellow))
            .bg(Color::Named(Color16::Blue))
            .attrs(Attributes::BOLD);
        assert_eq!(s.fg, Color::Named(Color16::Yellow));
        assert_eq!(s.bg, Color::Named(Color16::Blue));
        assert!(s.attrs.contains(Attributes::BOLD));
    }
}
