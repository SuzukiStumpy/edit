//! A scroll-bar indicator (TurboVision's `TScrollBar`), vertical or horizontal.
//!
//! A drawn indicator only: a pair of arrows, a track, and a thumb whose position
//! reflects how far a viewport has scrolled. Dragging it with the mouse is Phase
//! 9 (ADR 0007); for now a [`ListBox`](super::ListBox) draws a vertical one to
//! show where its selection sits, and the editor draws both along its window
//! frame to show its position in a longer/wider document.

use crate::canvas::Canvas;
use crate::cell::Cell;
use crate::color::Style;
use crate::geometry::{Point, Rect};
use crate::view::View;

const UP: char = '▲';
const DOWN: char = '▼';
const LEFT: char = '◄';
const RIGHT: char = '►';
const TRACK: char = '▒';
const THUMB: char = '█';

/// Which way a [`ScrollBar`] runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Runs top-to-bottom, one column wide (▲/▼ arrows).
    Vertical,
    /// Runs left-to-right, one row tall (◄/► arrows).
    Horizontal,
}

/// A scroll bar one cell thick, running the length of its [`bounds`](ScrollBar::bounds).
pub struct ScrollBar {
    bounds: Rect,
    orientation: Orientation,
    total: usize,
    visible: usize,
    pos: usize,
    style: Style,
}

impl ScrollBar {
    /// Creates a vertical scroll bar at `bounds` drawn in `style`, initially empty.
    pub fn new(bounds: Rect, style: Style) -> Self {
        Self::with_orientation(bounds, style, Orientation::Vertical)
    }

    /// Creates a horizontal scroll bar at `bounds` drawn in `style`.
    pub fn horizontal(bounds: Rect, style: Style) -> Self {
        Self::with_orientation(bounds, style, Orientation::Horizontal)
    }

    /// Creates a scroll bar with an explicit [`Orientation`].
    pub fn with_orientation(bounds: Rect, style: Style, orientation: Orientation) -> Self {
        Self {
            bounds,
            orientation,
            total: 0,
            visible: 1,
            pos: 0,
            style,
        }
    }

    /// Sets the range it reflects: `total` items, `visible` of them on screen at
    /// once, the topmost being item `pos`.
    pub fn set_metrics(&mut self, total: usize, visible: usize, pos: usize) {
        self.total = total;
        self.visible = visible.max(1);
        self.pos = pos;
    }

    /// The thumb's row offset within a track of `track_len` cells.
    fn thumb_offset(&self, track_len: i16) -> i16 {
        if track_len <= 1 {
            return 0;
        }
        let max_pos = self.total.saturating_sub(self.visible);
        if max_pos == 0 {
            return 0;
        }
        let pos = self.pos.min(max_pos);
        let span = track_len as usize - 1;
        ((pos * span + max_pos / 2) / max_pos) as i16
    }
}

impl View for ScrollBar {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn draw(&self, canvas: &mut Canvas) {
        let size = canvas.bounds().size();
        // The bar's length along its run; the cross-axis is one cell thick.
        let len = match self.orientation {
            Orientation::Vertical => size.height,
            Orientation::Horizontal => size.width,
        };
        if len <= 0 {
            return;
        }
        // Map an offset along the run to a local point on the (one-cell) cross-axis.
        let at = |i: i16| match self.orientation {
            Orientation::Vertical => Point::new(0, i),
            Orientation::Horizontal => Point::new(i, 0),
        };
        let (start_arrow, end_arrow) = match self.orientation {
            Orientation::Vertical => (UP, DOWN),
            Orientation::Horizontal => (LEFT, RIGHT),
        };
        if len == 1 {
            canvas.set(at(0), Cell::from_char(THUMB, self.style));
            return;
        }
        canvas.set(at(0), Cell::from_char(start_arrow, self.style));
        canvas.set(at(len - 1), Cell::from_char(end_arrow, self.style));
        let track_len = len - 2;
        for i in 1..len - 1 {
            canvas.set(at(i), Cell::from_char(TRACK, self.style));
        }
        if track_len >= 1 {
            let t = self.thumb_offset(track_len);
            canvas.set(at(1 + t), Cell::from_char(THUMB, self.style));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::color::Style;
    use crate::geometry::Size;

    fn glyph(buf: &Buffer, y: i16) -> String {
        buf.get(Point::new(0, y)).unwrap().grapheme().to_string()
    }

    fn render(bar: &ScrollBar, h: i16) -> Buffer {
        let mut buf = Buffer::new(Size::new(1, h));
        let mut canvas = Canvas::new(&mut buf);
        bar.draw(&mut canvas);
        buf
    }

    #[test]
    fn arrows_top_and_bottom() {
        let bar = ScrollBar::new(
            Rect::from_origin_size(Point::new(0, 0), Size::new(1, 6)),
            Style::new(),
        );
        let buf = render(&bar, 6);
        assert_eq!(glyph(&buf, 0), "▲");
        assert_eq!(glyph(&buf, 5), "▼");
    }

    #[test]
    fn horizontal_bar_has_left_right_arrows_and_a_tracking_thumb() {
        let mut bar = ScrollBar::horizontal(
            Rect::from_origin_size(Point::new(0, 0), Size::new(6, 1)),
            Style::new(),
        );
        bar.set_metrics(10, 4, 6); // scrolled fully right
        let mut buf = Buffer::new(Size::new(6, 1));
        let mut canvas = Canvas::new(&mut buf);
        bar.draw(&mut canvas);
        let glyph = |x: i16| buf.get(Point::new(x, 0)).unwrap().grapheme().to_string();
        assert_eq!(glyph(0), "◄");
        assert_eq!(glyph(5), "►");
        assert_eq!(glyph(4), "█", "thumb sits just before the right arrow");
    }

    #[test]
    fn thumb_tracks_the_scroll_position() {
        let mut bar = ScrollBar::new(
            Rect::from_origin_size(Point::new(0, 0), Size::new(1, 6)),
            Style::new(),
        );
        // 10 items, 4 visible: track is rows 1..5 (4 cells).
        bar.set_metrics(10, 4, 0);
        // At the top the thumb sits just under the up arrow (row 1).
        assert_eq!(glyph(&render(&bar, 6), 1), "█");
        // At the bottom (pos == total - visible) it sits just above the down arrow.
        bar.set_metrics(10, 4, 6);
        assert_eq!(glyph(&render(&bar, 6), 4), "█");
    }
}
