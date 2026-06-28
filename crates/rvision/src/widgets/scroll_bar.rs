//! A vertical scroll-bar indicator (TurboVision's `TScrollBar`).
//!
//! A drawn indicator only: up/down arrows, a track, and a thumb whose position
//! reflects how far a viewport has scrolled. Dragging it with the mouse is Phase
//! 9 (ADR 0007); for now a [`ListBox`](super::ListBox) draws one to show where
//! its selection sits in a longer list.

use crate::canvas::Canvas;
use crate::cell::Cell;
use crate::color::Style;
use crate::geometry::{Point, Rect};
use crate::view::View;

const UP: char = '▲';
const DOWN: char = '▼';
const TRACK: char = '▒';
const THUMB: char = '█';

/// A vertical scroll bar one column wide.
pub struct ScrollBar {
    bounds: Rect,
    total: usize,
    visible: usize,
    pos: usize,
    style: Style,
}

impl ScrollBar {
    /// Creates a scroll bar at `bounds` drawn in `style`, initially empty.
    pub fn new(bounds: Rect, style: Style) -> Self {
        Self {
            bounds,
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
        let h = canvas.bounds().height();
        if h <= 0 {
            return;
        }
        if h == 1 {
            canvas.set(Point::new(0, 0), Cell::from_char(THUMB, self.style));
            return;
        }
        canvas.set(Point::new(0, 0), Cell::from_char(UP, self.style));
        canvas.set(Point::new(0, h - 1), Cell::from_char(DOWN, self.style));
        let track_len = h - 2;
        for y in 1..h - 1 {
            canvas.set(Point::new(0, y), Cell::from_char(TRACK, self.style));
        }
        if track_len >= 1 {
            let t = self.thumb_offset(track_len);
            canvas.set(Point::new(0, 1 + t), Cell::from_char(THUMB, self.style));
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
