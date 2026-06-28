//! The screen buffer: a grid of [`Cell`]s that everything draws into.
//!
//! Draw primitives clip silently to the buffer bounds (ADR 0002). A backend later
//! diffs two buffers and flushes only the cells that changed.

use crate::cell::{Cell, Grapheme};
use crate::color::Style;
use crate::geometry::{Point, Rect, Size};
use core::fmt::Write as _;
use unicode_segmentation::UnicodeSegmentation;

/// A grid of [`Cell`]s, stored row-major.
#[derive(Debug, Clone)]
pub struct Buffer {
    size: Size,
    cells: Vec<Cell>,
}

impl Buffer {
    /// Creates a buffer of `size` filled with blank, default-styled cells. A
    /// non-positive dimension is treated as zero.
    pub fn new(size: Size) -> Self {
        let width = size.width.max(0);
        let height = size.height.max(0);
        let count = width as usize * height as usize;
        Self {
            size: Size::new(width, height),
            cells: vec![Cell::default(); count],
        }
    }

    /// The buffer's size.
    pub fn size(&self) -> Size {
        self.size
    }

    /// The buffer's width in columns.
    pub fn width(&self) -> i16 {
        self.size.width
    }

    /// The buffer's height in rows.
    pub fn height(&self) -> i16 {
        self.size.height
    }

    /// The buffer's full area as a rectangle at the origin.
    pub fn bounds(&self) -> Rect {
        Rect::from_origin_size(Point::new(0, 0), self.size)
    }

    /// Returns whether `p` is within the buffer.
    pub fn in_bounds(&self, p: Point) -> bool {
        p.x >= 0 && p.y >= 0 && p.x < self.size.width && p.y < self.size.height
    }

    /// Returns the flat index for `p`, or `None` if out of bounds.
    fn index(&self, p: Point) -> Option<usize> {
        if self.in_bounds(p) {
            Some(p.y as usize * self.size.width as usize + p.x as usize)
        } else {
            None
        }
    }

    /// Returns the cell at `p`, or `None` if out of bounds.
    pub fn get(&self, p: Point) -> Option<&Cell> {
        self.index(p).map(|i| &self.cells[i])
    }

    /// Writes `cell` at `p`, silently ignoring out-of-bounds writes. A wide
    /// (two-column) cell also writes a [`Cell::continuation`] at `p.x + 1`; if a
    /// wide cell would overflow the right edge, a blank is written instead.
    pub fn set(&mut self, p: Point, cell: Cell) {
        let Some(i) = self.index(p) else {
            return;
        };
        if cell.width() == 2 {
            if let Some(continuation) = self.index(p.offset(1, 0)) {
                let style = cell.style();
                self.cells[i] = cell;
                self.cells[continuation] = Cell::continuation(style);
            } else {
                self.cells[i] = Cell::blank(cell.style());
            }
        } else {
            self.cells[i] = cell;
        }
    }

    /// Writes the grapheme clusters of `s` left-to-right from `at`, each as one
    /// cell with `style`, advancing by each grapheme's display width and clipping
    /// at the buffer edge. Returns the column just past the last cell written.
    pub fn put_str(&mut self, at: Point, s: &str, style: Style) -> i16 {
        let mut x = at.x;
        if at.y < 0 || at.y >= self.size.height {
            return x;
        }
        for cluster in s.graphemes(true) {
            if x >= self.size.width {
                break;
            }
            let cell = Cell::new(Grapheme::new(cluster), style);
            let width = cell.width() as i16;
            // Never split a wide grapheme across the right edge.
            if width == 2 && x + 1 >= self.size.width {
                break;
            }
            self.set(Point::new(x, at.y), cell);
            x += width.max(1);
        }
        x
    }

    /// Fills `area` with clones of `cell`, clipped to the buffer.
    pub fn fill(&mut self, area: Rect, cell: &Cell) {
        let Some(clipped) = area.intersection(self.bounds()) else {
            return;
        };
        let br = clipped.bottom_right();
        for y in clipped.origin().y..br.y {
            for x in clipped.origin().x..br.x {
                self.set(Point::new(x, y), cell.clone());
            }
        }
    }

    /// Draws a single-line box border around `area` using Unicode box-drawing
    /// characters. Areas smaller than the border degrade gracefully (no panic);
    /// all writes clip to the buffer.
    pub fn draw_box(&mut self, area: Rect, style: Style) {
        if area.is_empty() {
            return;
        }
        let left = area.origin().x;
        let top = area.origin().y;
        let br = area.bottom_right();
        let right = br.x - 1;
        let bottom = br.y - 1;

        let horizontal = Cell::from_char('─', style);
        let vertical = Cell::from_char('│', style);

        for x in left..=right {
            self.set(Point::new(x, top), horizontal.clone());
            self.set(Point::new(x, bottom), horizontal.clone());
        }
        for y in top..=bottom {
            self.set(Point::new(left, y), vertical.clone());
            self.set(Point::new(right, y), vertical.clone());
        }
        // Corners overwrite the edge characters.
        self.set(Point::new(left, top), Cell::from_char('┌', style));
        self.set(Point::new(right, top), Cell::from_char('┐', style));
        self.set(Point::new(left, bottom), Cell::from_char('└', style));
        self.set(Point::new(right, bottom), Cell::from_char('┘', style));
    }

    /// Renders the buffer as text: one line per row (cell graphemes
    /// concatenated), rows joined by `\n`. Styles are not represented; this is
    /// for snapshots and debugging.
    pub fn to_text(&self) -> String {
        let mut out =
            String::with_capacity((self.size.width as usize + 1) * self.size.height as usize);
        for y in 0..self.size.height {
            if y > 0 {
                out.push('\n');
            }
            for x in 0..self.size.width {
                if let Some(cell) = self.get(Point::new(x, y)) {
                    let _ = write!(out, "{}", cell.grapheme());
                }
            }
        }
        out
    }

    /// Returns the cells of `self` that differ from `previous`, in row-major
    /// order, as `(position, cell)` pairs. Intended for flushing only changed
    /// cells to a backend (ADR 0002); assumes both buffers are the same size.
    pub fn diff<'a>(&'a self, previous: &Buffer) -> Vec<(Point, &'a Cell)> {
        let mut changes = Vec::new();
        for y in 0..self.size.height {
            for x in 0..self.size.width {
                let p = Point::new(x, y);
                let cell = self.get(p);
                if cell != previous.get(p) {
                    if let Some(cell) = cell {
                        changes.push((p, cell));
                    }
                }
            }
        }
        changes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Style;

    // Tracer bullet: a fresh buffer has the right shape and blank contents.
    #[test]
    fn new_has_dimensions_and_blank_cells() {
        let buf = Buffer::new(Size::new(4, 3));
        assert_eq!(buf.size(), Size::new(4, 3));
        assert_eq!(buf.width(), 4);
        assert_eq!(buf.height(), 3);

        assert!(buf.in_bounds(Point::new(0, 0)));
        assert!(buf.in_bounds(Point::new(3, 2)));
        assert!(!buf.in_bounds(Point::new(4, 0)));
        assert!(!buf.in_bounds(Point::new(0, 3)));
        assert!(!buf.in_bounds(Point::new(-1, 0)));

        assert_eq!(
            buf.get(Point::new(0, 0)).unwrap().grapheme().to_string(),
            " "
        );
        assert!(buf.get(Point::new(4, 0)).is_none());
    }

    #[test]
    fn set_then_get_round_trips() {
        let mut buf = Buffer::new(Size::new(3, 2));
        let cell = Cell::from_char('X', Style::new());
        buf.set(Point::new(1, 1), cell.clone());
        assert_eq!(buf.get(Point::new(1, 1)), Some(&cell));

        // Out-of-bounds set is a silent no-op.
        buf.set(Point::new(99, 99), Cell::from_char('!', Style::new()));
        assert!(buf.get(Point::new(99, 99)).is_none());
    }

    #[test]
    fn fill_paints_clipped_rect_only() {
        let mut buf = Buffer::new(Size::new(5, 4));
        let hash = Cell::from_char('#', Style::new());

        // Rect runs off the right/bottom edge; only the in-bounds part fills.
        buf.fill(
            Rect::from_origin_size(Point::new(3, 2), Size::new(10, 10)),
            &hash,
        );

        assert_eq!(buf.get(Point::new(3, 2)), Some(&hash));
        assert_eq!(buf.get(Point::new(4, 3)), Some(&hash));
        // Cells outside the rect remain blank.
        assert_eq!(
            buf.get(Point::new(2, 2)).unwrap().grapheme().to_string(),
            " "
        );
        assert_eq!(
            buf.get(Point::new(0, 0)).unwrap().grapheme().to_string(),
            " "
        );
    }

    #[test]
    fn put_str_writes_advances_and_clips() {
        let mut buf = Buffer::new(Size::new(6, 1));
        let end = buf.put_str(Point::new(1, 0), "Hi!", Style::new());
        assert_eq!(end, 4);
        assert_eq!(
            buf.get(Point::new(0, 0)).unwrap().grapheme().to_string(),
            " "
        );
        assert_eq!(
            buf.get(Point::new(1, 0)).unwrap().grapheme().to_string(),
            "H"
        );
        assert_eq!(
            buf.get(Point::new(2, 0)).unwrap().grapheme().to_string(),
            "i"
        );
        assert_eq!(
            buf.get(Point::new(3, 0)).unwrap().grapheme().to_string(),
            "!"
        );

        // Clipping: only what fits before the right edge is written.
        let mut narrow = Buffer::new(Size::new(3, 1));
        let end = narrow.put_str(Point::new(0, 0), "ABCDEF", Style::new());
        assert_eq!(end, 3);
        assert_eq!(
            narrow.get(Point::new(2, 0)).unwrap().grapheme().to_string(),
            "C"
        );
    }

    #[test]
    fn put_str_wide_grapheme_consumes_two_columns() {
        let mut buf = Buffer::new(Size::new(5, 1));
        let end = buf.put_str(Point::new(0, 0), "世x", Style::new());
        assert_eq!(
            buf.get(Point::new(0, 0)).unwrap().grapheme().to_string(),
            "世"
        );
        // The next column is a zero-width continuation.
        assert_eq!(buf.get(Point::new(1, 0)).unwrap().width(), 0);
        assert_eq!(
            buf.get(Point::new(2, 0)).unwrap().grapheme().to_string(),
            "x"
        );
        assert_eq!(end, 3);
    }

    #[test]
    fn draw_box_renders_border() {
        let mut buf = Buffer::new(Size::new(4, 3));
        buf.draw_box(
            Rect::from_origin_size(Point::new(0, 0), Size::new(4, 3)),
            Style::new(),
        );
        let g = |x: i16, y: i16| buf.get(Point::new(x, y)).unwrap().grapheme().to_string();
        assert_eq!(g(0, 0), "┌");
        assert_eq!(g(3, 0), "┐");
        assert_eq!(g(0, 2), "└");
        assert_eq!(g(3, 2), "┘");
        assert_eq!(g(1, 0), "─");
        assert_eq!(g(0, 1), "│");
        assert_eq!(g(3, 1), "│");
        // The interior is left untouched.
        assert_eq!(g(1, 1), " ");
    }

    #[test]
    fn draw_box_tiny_area_does_not_panic() {
        let mut buf = Buffer::new(Size::new(2, 2));
        buf.draw_box(
            Rect::from_origin_size(Point::new(0, 0), Size::new(1, 1)),
            Style::new(),
        );
    }

    #[test]
    fn snapshot_box_with_title_and_text() {
        let mut buf = Buffer::new(Size::new(12, 4));
        buf.draw_box(
            Rect::from_origin_size(Point::new(0, 0), Size::new(12, 4)),
            Style::new(),
        );
        buf.put_str(Point::new(2, 0), " Edit ", Style::new());
        buf.put_str(Point::new(2, 2), "hi", Style::new());
        insta::assert_snapshot!(buf.to_text());
    }

    #[test]
    fn diff_reports_only_changed_cells() {
        let a = Buffer::new(Size::new(3, 2));
        let mut b = a.clone();
        b.set(Point::new(0, 0), Cell::from_char('Q', Style::new()));
        b.set(Point::new(2, 1), Cell::from_char('Z', Style::new()));

        let changes = b.diff(&a);
        assert_eq!(changes.len(), 2);
        // Row-major order.
        assert_eq!(changes[0].0, Point::new(0, 0));
        assert_eq!(changes[0].1.grapheme().to_string(), "Q");
        assert_eq!(changes[1].0, Point::new(2, 1));
        assert_eq!(changes[1].1.grapheme().to_string(), "Z");

        // Identical buffers differ nowhere.
        assert!(a.diff(&a).is_empty());
    }
}
