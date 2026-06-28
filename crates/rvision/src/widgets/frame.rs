//! A window border: a single- or double-line box with a title and the
//! close/zoom glyphs on its top edge.
//!
//! `Frame` is a drawing helper, not an independent [`View`](crate::view::View):
//! it always paints the *whole* canvas it is handed — a window's outer rectangle —
//! so it has no bounds of its own. [`Window`](super::Window) owns one and draws it
//! before its interior.

use crate::canvas::Canvas;
use crate::cell::Cell;
use crate::color::Style;
use crate::geometry::{Point, Rect};

/// The six glyphs of a box border: the four corners and the two edges.
struct BorderGlyphs {
    top_left: char,
    top_right: char,
    bottom_left: char,
    bottom_right: char,
    horizontal: char,
    vertical: char,
}

/// Single-line border (inactive windows).
const SINGLE: BorderGlyphs = BorderGlyphs {
    top_left: '┌',
    top_right: '┐',
    bottom_left: '└',
    bottom_right: '┘',
    horizontal: '─',
    vertical: '│',
};

/// Double-line border (the active window stands out, as in TurboVision).
const DOUBLE: BorderGlyphs = BorderGlyphs {
    top_left: '╔',
    top_right: '╗',
    bottom_left: '╚',
    bottom_right: '╝',
    horizontal: '═',
    vertical: '║',
};

/// The close glyph drawn near the top-left corner (decorative until Phase 9).
const CLOSE: &str = "[■]";
/// The zoom glyph drawn near the top-right corner (decorative until Phase 9).
const ZOOM: &str = "[↑]";

/// A window frame: border, centred title, and close/zoom glyphs.
pub struct Frame {
    title: String,
    active: bool,
    style: Style,
    title_style: Style,
}

impl Frame {
    /// Creates an (inactive) frame titled `title`, with `style` for the border and
    /// glyphs and `title_style` for the title text.
    pub fn new(title: &str, style: Style, title_style: Style) -> Self {
        Self {
            title: title.to_string(),
            active: false,
            style,
            title_style,
        }
    }

    /// Marks the frame active (a doubled border) or not (a single one).
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Sets the active flag in place — the desktop calls this through its
    /// [`Window`](super::Window) as the focused window changes.
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    /// Draws the frame over the whole canvas it is handed. Degrades without panic
    /// for areas too small to hold a box (anything narrower or shorter than 2).
    pub fn draw(&self, canvas: &mut Canvas) {
        let area = canvas.bounds();
        let (w, h) = (area.width(), area.height());
        if w < 2 || h < 2 {
            return;
        }
        let g = if self.active { &DOUBLE } else { &SINGLE };
        self.draw_border(canvas, area, g);

        // The close/zoom glyphs sit just inside the corners; only drawn when the
        // frame is wide enough, so a narrow one keeps a clean border instead of a
        // clipped half-glyph. The title is centred in the span *between* them (or
        // the whole top edge when they are absent) and truncated to fit, so it can
        // never overdraw a glyph.
        let top = 0;
        let glyphs = w >= 10;
        let (left, right) = if glyphs {
            let zoom_len = ZOOM.chars().count() as i16;
            canvas.put_str(Point::new(2, top), CLOSE, self.style);
            canvas.put_str(Point::new(w - 1 - zoom_len, top), ZOOM, self.style);
            (2 + CLOSE.chars().count() as i16, w - 1 - zoom_len)
        } else {
            (1, w - 1)
        };
        self.draw_title(canvas, top, left, right);
    }

    /// Draws the title centred in the half-open column span `[left, right)`,
    /// truncated to fit. Does nothing if the span or the title is empty.
    fn draw_title(&self, canvas: &mut Canvas, row: i16, left: i16, right: i16) {
        if self.title.is_empty() || right <= left {
            return;
        }
        let span = (right - left) as usize;
        let label = format!(" {} ", self.title);
        let shown: String = label.chars().take(span).collect();
        let len = shown.chars().count() as i16;
        let x = left + (right - left - len) / 2;
        canvas.put_str(Point::new(x, row), &shown, self.title_style);
    }

    /// Strokes the four edges and overwrites the corners.
    fn draw_border(&self, canvas: &mut Canvas, area: Rect, g: &BorderGlyphs) {
        let br = area.bottom_right();
        let (left, top) = (area.origin().x, area.origin().y);
        let (right, bottom) = (br.x - 1, br.y - 1);

        let h = Cell::from_char(g.horizontal, self.style);
        let v = Cell::from_char(g.vertical, self.style);
        for x in left..=right {
            canvas.set(Point::new(x, top), h.clone());
            canvas.set(Point::new(x, bottom), h.clone());
        }
        for y in top..=bottom {
            canvas.set(Point::new(left, y), v.clone());
            canvas.set(Point::new(right, y), v.clone());
        }
        canvas.set(
            Point::new(left, top),
            Cell::from_char(g.top_left, self.style),
        );
        canvas.set(
            Point::new(right, top),
            Cell::from_char(g.top_right, self.style),
        );
        canvas.set(
            Point::new(left, bottom),
            Cell::from_char(g.bottom_left, self.style),
        );
        canvas.set(
            Point::new(right, bottom),
            Cell::from_char(g.bottom_right, self.style),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::geometry::Size;

    fn render(frame: &Frame, w: i16, h: i16) -> String {
        let mut buf = Buffer::new(Size::new(w, h));
        let mut canvas = Canvas::new(&mut buf);
        frame.draw(&mut canvas);
        buf.to_text()
    }

    // Tracer bullet: an inactive frame is a single-line box with a centred title
    // and the close/zoom glyphs on the top edge.
    #[test]
    fn snapshot_inactive_frame() {
        let frame = Frame::new("Untitled", Style::new(), Style::new());
        insta::assert_snapshot!(render(&frame, 20, 5));
    }

    #[test]
    fn snapshot_active_frame_has_doubled_border() {
        let frame = Frame::new("Untitled", Style::new(), Style::new()).active(true);
        insta::assert_snapshot!(render(&frame, 20, 5));
    }

    #[test]
    fn narrow_frame_drops_glyphs_but_keeps_a_clean_box() {
        // Too narrow (8 < 10) for close/zoom; still a tidy single-line box.
        let frame = Frame::new("X", Style::new(), Style::new());
        insta::assert_snapshot!(render(&frame, 8, 3));
    }

    #[test]
    fn tiny_area_does_not_panic() {
        let frame = Frame::new("nope", Style::new(), Style::new());
        // 1-wide / 1-tall: below the box minimum; draws nothing, no panic.
        assert_eq!(render(&frame, 1, 4), " \n \n \n ");
        assert_eq!(render(&frame, 4, 1), "    ");
    }
}
