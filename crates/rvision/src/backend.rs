//! The output seam: a [`Backend`] takes a finished frame and makes it visible,
//! emitting only the cells that changed since the last frame.
//!
//! This is the framework's only path to the screen (ADR 0002). [`TestBackend`]
//! drives tests headlessly; the real `CrosstermBackend` arrives in Phase 2, as
//! does the input half of the seam (`EventSource`, which carries `Event`).

use crate::buffer::Buffer;
use crate::geometry::Size;

/// A target the framework presents finished frames to.
///
/// The framework draws into an in-memory back [`Buffer`]; the backend holds the
/// front (on-screen) buffer, diffs the incoming frame against it, and emits only
/// the changed cells (ADR 0002).
pub trait Backend {
    /// The size of the surface being presented to.
    fn size(&self) -> Size;

    /// Presents a finished frame: diffs it against the current screen and makes
    /// the changed cells visible. Assumes `frame.size() == self.size()`.
    fn present(&mut self, frame: &Buffer);
}

/// A headless [`Backend`] for tests: keeps the "screen" in memory and records
/// what the most recent [`present`](Backend::present) would have changed.
#[derive(Debug, Clone)]
pub struct TestBackend {
    screen: Buffer,
    last_changes: usize,
    presents: usize,
}

impl TestBackend {
    /// Creates a blank, default-styled test screen of `size`.
    pub fn new(size: Size) -> Self {
        Self {
            screen: Buffer::new(size),
            last_changes: 0,
            presents: 0,
        }
    }

    /// The current on-screen contents.
    pub fn screen(&self) -> &Buffer {
        &self.screen
    }

    /// The current screen as text (rows joined by `'\n'`).
    pub fn to_text(&self) -> String {
        self.screen.to_text()
    }

    /// The number of cells emitted by the most recent `present`.
    pub fn last_changes(&self) -> usize {
        self.last_changes
    }

    /// The number of `present` calls so far.
    pub fn presents(&self) -> usize {
        self.presents
    }
}

impl Backend for TestBackend {
    fn size(&self) -> Size {
        self.screen.size()
    }

    fn present(&mut self, frame: &Buffer) {
        self.last_changes = frame.diff(&self.screen).len();
        self.screen = frame.clone();
        self.presents += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell::Cell;
    use crate::color::Style;

    // Tracer bullet: a fresh backend reports its size and a blank screen, and a
    // presented frame becomes the screen.
    #[test]
    fn present_adopts_the_frame() {
        let mut backend = TestBackend::new(Size::new(4, 2));
        assert_eq!(backend.size(), Size::new(4, 2));
        assert_eq!(backend.to_text(), "    \n    ");

        let mut frame = Buffer::new(Size::new(4, 2));
        frame.put_str(crate::geometry::Point::new(0, 0), "hi", Style::new());
        backend.present(&frame);

        assert_eq!(backend.to_text(), "hi  \n    ");
        assert_eq!(backend.presents(), 1);
    }

    #[test]
    fn re_presenting_the_same_frame_changes_nothing() {
        let mut backend = TestBackend::new(Size::new(8, 3));
        let mut frame = Buffer::new(Size::new(8, 3));
        frame.draw_box(frame.bounds(), Style::new());

        backend.present(&frame);
        let first = backend.last_changes();
        assert!(first > 0, "first present should change cells");

        backend.present(&frame); // identical frame
        assert_eq!(backend.last_changes(), 0, "minimal update: nothing changed");
        assert_eq!(backend.presents(), 2);
    }

    #[test]
    fn a_single_cell_edit_emits_one_change() {
        let mut backend = TestBackend::new(Size::new(5, 1));
        let blank = Buffer::new(Size::new(5, 1));
        backend.present(&blank);

        let mut frame = blank.clone();
        frame.set(
            crate::geometry::Point::new(2, 0),
            Cell::from_char('X', Style::new()),
        );
        backend.present(&frame);

        assert_eq!(backend.last_changes(), 1);
        assert_eq!(backend.to_text(), "  X  ");
    }
}
