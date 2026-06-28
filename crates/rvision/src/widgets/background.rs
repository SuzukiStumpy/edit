//! The desktop backdrop: a [`View`] that fills its area with one repeated cell.

use crate::canvas::Canvas;
use crate::cell::Cell;
use crate::geometry::Rect;
use crate::view::View;

/// A solid backdrop. The simplest chrome leaf: it fills the whole canvas it is
/// handed with clones of one cell (classically the `░` shade in
/// [`Role::DesktopBackground`](crate::theme::Role::DesktopBackground)) and ignores
/// every event.
///
/// It fills the canvas it is *given*, not the rectangle in its own
/// [`bounds`](View::bounds): the application shell sizes that canvas to the live
/// terminal (ADR 0016), so the backdrop covers a resized desktop without being
/// reconstructed.
pub struct Background {
    bounds: Rect,
    cell: Cell,
}

impl Background {
    /// Creates a backdrop occupying `bounds`, painted with clones of `cell`.
    pub fn new(bounds: Rect, cell: Cell) -> Self {
        Self { bounds, cell }
    }
}

impl View for Background {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn draw(&self, canvas: &mut Canvas) {
        let area = canvas.bounds();
        canvas.fill(area, &self.cell);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::color::Style;
    use crate::geometry::{Point, Size};

    fn rect(x: i16, y: i16, w: i16, h: i16) -> Rect {
        Rect::from_origin_size(Point::new(x, y), Size::new(w, h))
    }

    // Tracer bullet: a backdrop paints its glyph across the whole surface it is
    // handed, regardless of the rect it was constructed with.
    #[test]
    fn fills_the_canvas_it_is_given() {
        let bg = Background::new(rect(0, 0, 3, 2), Cell::from_char('░', Style::new()));
        let mut buf = Buffer::new(Size::new(4, 3));
        let mut canvas = Canvas::new(&mut buf);
        // Hand it a 4x3 surface even though it was made for 3x2: it fills 4x3.
        bg.draw(&mut canvas);
        assert_eq!(buf.to_text(), "░░░░\n░░░░\n░░░░");
    }

    #[test]
    fn fill_is_confined_to_the_assigned_child_canvas() {
        let bg = Background::new(rect(0, 0, 2, 1), Cell::from_char('#', Style::new()));
        let mut buf = Buffer::new(Size::new(5, 3));
        let mut root = Canvas::new(&mut buf);
        let mut sub = root.child(rect(1, 1, 2, 1));
        bg.draw(&mut sub);
        // Only the 2x1 sub-canvas is painted; everything around it stays blank.
        assert_eq!(buf.to_text(), "     \n ##  \n     ");
    }

    #[test]
    fn is_not_focusable() {
        let bg = Background::new(rect(0, 0, 1, 1), Cell::default());
        assert!(!bg.focusable());
        assert_eq!(bg.bounds(), rect(0, 0, 1, 1));
    }
}
