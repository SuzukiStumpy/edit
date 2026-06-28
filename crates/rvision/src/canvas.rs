//! A translating, clipping draw surface over a [`Buffer`] (ADR 0015).
//!
//! A view draws in its own local `(0, 0)`-origin coordinates; the [`Canvas`]
//! adds the view's absolute offset and confines every write to the view's box
//! (and every ancestor's). [`Canvas::child`] carves a sub-surface for a child
//! view, so a parent group hands each child a canvas already positioned and
//! clipped — the draw half of the seam the `view::View` trait sits on.
//!
//! The primitives mirror [`Buffer`]'s so draw code reads the same whether it
//! targets a buffer directly (as Phase 1 tests do) or a canvas. Unlike a
//! `Buffer`, a `Canvas` owns no cells: it borrows one and writes through it.

use crate::buffer::Buffer;
use crate::cell::{self, Cell};
use crate::color::Style;
use crate::geometry::{Point, Rect, Size};

/// A translating, clipping view onto a [`Buffer`].
///
/// Local coordinates have their origin at the surface's top-left; a write at
/// local `p` lands at absolute `offset + p` and only if it falls inside `clip`
/// (ADR 0015). `size` is the surface's nominal area — the view's own box — which
/// can be larger than `clip` when an ancestor clips it.
pub struct Canvas<'a> {
    buffer: &'a mut Buffer,
    offset: Point,
    size: Size,
    clip: Rect,
}

impl<'a> Canvas<'a> {
    /// Creates a root canvas covering the whole `buffer`: local `(0, 0)` is the
    /// buffer's top-left and the clip is the entire buffer.
    pub fn new(buffer: &'a mut Buffer) -> Self {
        let size = buffer.size();
        let clip = buffer.bounds();
        Self {
            buffer,
            offset: Point::new(0, 0),
            size,
            clip,
        }
    }

    /// Carves a sub-canvas for `area`, expressed in **this** canvas's local
    /// coordinates. The child's local `(0, 0)` is `area`'s origin; its clip is
    /// the overlap of this canvas's clip and the child's absolute box, so a child
    /// reaching past its parent is silently trimmed and one wholly outside draws
    /// nothing.
    pub fn child(&mut self, area: Rect) -> Canvas<'_> {
        let offset = self.offset.offset(area.origin().x, area.origin().y);
        let absolute = Rect::from_origin_size(offset, area.size());
        let clip = self
            .clip
            .intersection(absolute)
            .unwrap_or_else(|| Rect::from_origin_size(offset, Size::new(0, 0)));
        Canvas {
            buffer: &mut *self.buffer,
            offset,
            size: area.size(),
            clip,
        }
    }

    /// The surface's nominal size in local coordinates (the view's own area).
    pub fn size(&self) -> Size {
        self.size
    }

    /// The surface's local bounds: `(0, 0)` to [`size`](Self::size).
    pub fn bounds(&self) -> Rect {
        Rect::from_origin_size(Point::new(0, 0), self.size)
    }

    /// Writes `cell` at local point `at`, silently dropping it if it falls
    /// outside the clip. A wide (two-column) cell whose continuation column would
    /// land outside the clip is replaced by a blank, mirroring [`Buffer`]'s
    /// right-edge rule so no half-glyph spills across the boundary.
    pub fn set(&mut self, at: Point, cell: Cell) {
        let absolute = self.offset.offset(at.x, at.y);
        if !self.clip.contains(absolute) {
            return;
        }
        if cell.width() == 2 && !self.clip.contains(absolute.offset(1, 0)) {
            self.buffer.set(absolute, Cell::blank(cell.style()));
        } else {
            self.buffer.set(absolute, cell);
        }
    }

    /// Writes the grapheme clusters of `s` left-to-right from local `at`, each as
    /// one cell with `style`, advancing by display width and stopping at the
    /// surface's right edge (and, per [`set`](Self::set), at the clip). Returns
    /// the local column just past the last cell written, like
    /// [`Buffer::put_str`].
    pub fn put_str(&mut self, at: Point, s: &str, style: Style) -> i16 {
        let mut x = at.x;
        if at.y < 0 || at.y >= self.size.height {
            return x;
        }
        for cell in cell::cells_of(s, style) {
            if x >= self.size.width {
                break;
            }
            let width = cell.width() as i16;
            // Never split a wide grapheme across the surface's right edge.
            if width == 2 && x + 1 >= self.size.width {
                break;
            }
            self.set(Point::new(x, at.y), cell);
            x += width.max(1);
        }
        x
    }

    /// Fills `area` (local coordinates) with clones of `cell`, clipped to the
    /// surface bounds and the clip.
    pub fn fill(&mut self, area: Rect, cell: &Cell) {
        let Some(local) = area.intersection(self.bounds()) else {
            return;
        };
        let br = local.bottom_right();
        for y in local.origin().y..br.y {
            for x in local.origin().x..br.x {
                self.set(Point::new(x, y), cell.clone());
            }
        }
    }

    /// Draws a single-line box border around `area` (local coordinates) using
    /// Unicode box-drawing characters. Degrades gracefully for tiny areas; all
    /// writes clip to the surface.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph(buf: &Buffer, x: i16, y: i16) -> String {
        buf.get(Point::new(x, y)).unwrap().grapheme().to_string()
    }

    // Tracer bullet: a root canvas maps local coordinates straight to the buffer.
    #[test]
    fn root_canvas_writes_one_to_one() {
        let mut buf = Buffer::new(Size::new(5, 3));
        let mut canvas = Canvas::new(&mut buf);
        assert_eq!(canvas.size(), Size::new(5, 3));
        canvas.set(Point::new(1, 2), Cell::from_char('X', Style::new()));
        assert_eq!(glyph(&buf, 1, 2), "X");
    }

    #[test]
    fn child_translates_local_origin_to_its_box() {
        let mut buf = Buffer::new(Size::new(10, 5));
        let mut root = Canvas::new(&mut buf);
        let mut child = root.child(Rect::from_origin_size(Point::new(3, 1), Size::new(4, 2)));
        assert_eq!(child.size(), Size::new(4, 2));
        // The child's local (0,0) is the box's top-left in the buffer.
        child.put_str(Point::new(0, 0), "ab", Style::new());
        assert_eq!(glyph(&buf, 3, 1), "a");
        assert_eq!(glyph(&buf, 4, 1), "b");
        // Nothing landed at the buffer origin.
        assert_eq!(glyph(&buf, 0, 0), " ");
    }

    #[test]
    fn child_clips_overlong_text_to_its_box_not_the_screen() {
        let mut buf = Buffer::new(Size::new(10, 1));
        let mut root = Canvas::new(&mut buf);
        let mut child = root.child(Rect::from_origin_size(Point::new(2, 0), Size::new(3, 1)));
        // The string is longer than the child; it must stop at the child's right
        // edge (col 4), never spilling into the rest of the buffer (cols 5+).
        let end = child.put_str(Point::new(0, 0), "ABCDEF", Style::new());
        assert_eq!(
            end, 3,
            "advance is in local coords and stops at the box width"
        );
        assert_eq!(glyph(&buf, 2, 0), "A");
        assert_eq!(glyph(&buf, 3, 0), "B");
        assert_eq!(glyph(&buf, 4, 0), "C");
        // No spill past the child's box.
        assert_eq!(glyph(&buf, 5, 0), " ");
    }

    #[test]
    fn nested_children_compose_offsets() {
        let mut buf = Buffer::new(Size::new(12, 6));
        let mut root = Canvas::new(&mut buf);
        let mut outer = root.child(Rect::from_origin_size(Point::new(2, 1), Size::new(8, 4)));
        let mut inner = outer.child(Rect::from_origin_size(Point::new(1, 1), Size::new(4, 2)));
        // Inner local (0,0) == buffer (3,2): outer (2,1) + inner (1,1).
        inner.set(Point::new(0, 0), Cell::from_char('Z', Style::new()));
        assert_eq!(glyph(&buf, 3, 2), "Z");
    }

    #[test]
    fn child_partly_off_parent_is_trimmed() {
        let mut buf = Buffer::new(Size::new(6, 2));
        let mut root = Canvas::new(&mut buf);
        // Child runs off the right edge of the buffer; its clip is trimmed there
        // even though its nominal size is wider.
        let mut child = root.child(Rect::from_origin_size(Point::new(4, 0), Size::new(5, 1)));
        assert_eq!(child.size(), Size::new(5, 1));
        child.put_str(Point::new(0, 0), "QQQQ", Style::new());
        assert_eq!(glyph(&buf, 4, 0), "Q");
        assert_eq!(glyph(&buf, 5, 0), "Q");
        // Cols 6+ do not exist; nothing panicked and nothing wrote out of bounds.
        assert!(buf.get(Point::new(6, 0)).is_none());
    }

    #[test]
    fn child_wholly_off_parent_draws_nothing() {
        let mut buf = Buffer::new(Size::new(5, 5));
        let mut root = Canvas::new(&mut buf);
        let mut child = root.child(Rect::from_origin_size(Point::new(10, 10), Size::new(3, 3)));
        child.fill(child.bounds(), &Cell::from_char('#', Style::new()));
        // The whole buffer is still blank.
        assert_eq!(buf.to_text().replace('\n', ""), " ".repeat(25));
    }

    #[test]
    fn wide_glyph_whose_continuation_leaves_the_clip_is_blanked() {
        // A child clipped narrower than its size (here by an ancestor box). A wide
        // glyph placed at the last clipped column would put its continuation cell
        // outside the clip — `set` blanks it rather than spilling a half-glyph.
        let mut buf = Buffer::new(Size::new(10, 1));
        let mut root = Canvas::new(&mut buf);
        let mut child = root.child(Rect::from_origin_size(Point::new(0, 0), Size::new(3, 1)));
        // Bypass put_str's own right-edge guard to exercise set's clip guard.
        child.set(Point::new(2, 0), Cell::from_char('世', Style::new()));
        assert_eq!(glyph(&buf, 2, 0), " ", "wide glyph at clip edge is blanked");
        assert_eq!(
            glyph(&buf, 3, 0),
            " ",
            "no continuation spilled past the clip"
        );
    }

    #[test]
    fn fill_is_confined_to_the_child_box() {
        let mut buf = Buffer::new(Size::new(6, 4));
        let mut root = Canvas::new(&mut buf);
        let mut child = root.child(Rect::from_origin_size(Point::new(1, 1), Size::new(3, 2)));
        child.fill(child.bounds(), &Cell::from_char('#', Style::new()));
        // Inside the box is filled...
        assert_eq!(glyph(&buf, 1, 1), "#");
        assert_eq!(glyph(&buf, 3, 2), "#");
        // ...everything around it is untouched.
        assert_eq!(glyph(&buf, 0, 0), " ");
        assert_eq!(glyph(&buf, 4, 1), " ");
        assert_eq!(glyph(&buf, 1, 3), " ");
    }

    #[test]
    fn snapshot_boxed_titled_child() {
        let mut buf = Buffer::new(Size::new(14, 5));
        let mut root = Canvas::new(&mut buf);
        let mut child = root.child(Rect::from_origin_size(Point::new(2, 1), Size::new(10, 3)));
        child.draw_box(child.bounds(), Style::new());
        child.put_str(Point::new(2, 0), " Hi ", Style::new());
        child.put_str(Point::new(2, 1), "edit", Style::new());
        insta::assert_snapshot!(buf.to_text());
    }
}
