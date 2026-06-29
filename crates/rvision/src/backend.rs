//! The seam between the framework and the outside world (ADR 0002): a
//! [`Backend`] takes a finished frame and makes it visible, emitting only the
//! cells that changed; an [`EventSource`] supplies input.
//!
//! [`TestBackend`] drives the output half of tests headlessly. The real
//! [`CrosstermBackend`](crate::crossterm_backend::CrosstermBackend) implements
//! both traits over a terminal and confines the one crossterm dependency
//! (ADR 0001).

use crate::buffer::Buffer;
use crate::event::Event;
use crate::geometry::Size;
use std::io;
use std::time::Duration;

/// A target the framework presents finished frames to.
///
/// The framework draws into an in-memory back [`Buffer`]; the backend holds the
/// front (on-screen) buffer, diffs the incoming frame against it, and emits only
/// the changed cells (ADR 0002).
pub trait Backend {
    /// The size of the surface being presented to.
    fn size(&self) -> Size;

    /// Presents a finished frame: diffs it against the current screen and makes
    /// the changed cells visible. Fallible because a real flush is terminal I/O.
    fn present(&mut self, frame: &Buffer) -> io::Result<()>;

    /// Copies `text` to the host's system clipboard, if the backend can reach it.
    /// The real backend emits an OSC 52 escape (ADR 0021); the default is a no-op,
    /// so a backend with no terminal (or no clipboard) simply drops the request.
    fn set_clipboard(&mut self, text: &str) -> io::Result<()> {
        let _ = text;
        Ok(())
    }
}

/// The input half of the seam: a source of [`Event`]s (ADR 0002, 0004).
pub trait EventSource {
    /// Blocks up to `timeout` for the next event. Returns `Ok(Some(event))` if one
    /// arrived, `Ok(None)` if the timeout elapsed first (the loop turns this into
    /// [`Event::Idle`], so the timeout is the idle/blink cadence), or `Err` on a
    /// real I/O error.
    fn poll_event(&mut self, timeout: Duration) -> io::Result<Option<Event>>;
}

/// A headless [`Backend`] for tests: keeps the "screen" in memory and records
/// what the most recent [`present`](Backend::present) would have changed.
#[derive(Debug, Clone)]
pub struct TestBackend {
    screen: Buffer,
    last_changes: usize,
    presents: usize,
    clipboard: Option<String>,
}

impl TestBackend {
    /// Creates a blank, default-styled test screen of `size`.
    pub fn new(size: Size) -> Self {
        Self {
            screen: Buffer::new(size),
            last_changes: 0,
            presents: 0,
            clipboard: None,
        }
    }

    /// The text most recently pushed to the system clipboard via
    /// [`set_clipboard`](Backend::set_clipboard), or `None` if none was (ADR 0021).
    pub fn clipboard(&self) -> Option<&str> {
        self.clipboard.as_deref()
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

    fn present(&mut self, frame: &Buffer) -> io::Result<()> {
        self.last_changes = frame.diff(&self.screen).len();
        self.screen = frame.clone();
        self.presents += 1;
        Ok(())
    }

    fn set_clipboard(&mut self, text: &str) -> io::Result<()> {
        self.clipboard = Some(text.to_string());
        Ok(())
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
        backend.present(&frame).unwrap();

        assert_eq!(backend.to_text(), "hi  \n    ");
        assert_eq!(backend.presents(), 1);
    }

    #[test]
    fn re_presenting_the_same_frame_changes_nothing() {
        let mut backend = TestBackend::new(Size::new(8, 3));
        let mut frame = Buffer::new(Size::new(8, 3));
        frame.draw_box(frame.bounds(), Style::new());

        backend.present(&frame).unwrap();
        let first = backend.last_changes();
        assert!(first > 0, "first present should change cells");

        backend.present(&frame).unwrap(); // identical frame
        assert_eq!(backend.last_changes(), 0, "minimal update: nothing changed");
        assert_eq!(backend.presents(), 2);
    }

    #[test]
    fn set_clipboard_records_the_last_text() {
        let mut backend = TestBackend::new(Size::new(4, 1));
        assert_eq!(backend.clipboard(), None, "nothing pushed yet");
        backend.set_clipboard("hello").unwrap();
        assert_eq!(backend.clipboard(), Some("hello"));
        backend.set_clipboard("world").unwrap();
        assert_eq!(backend.clipboard(), Some("world"), "keeps the most recent");
    }

    #[test]
    fn a_single_cell_edit_emits_one_change() {
        let mut backend = TestBackend::new(Size::new(5, 1));
        let blank = Buffer::new(Size::new(5, 1));
        backend.present(&blank).unwrap();

        let mut frame = blank.clone();
        frame.set(
            crate::geometry::Point::new(2, 0),
            Cell::from_char('X', Style::new()),
        );
        backend.present(&frame).unwrap();

        assert_eq!(backend.last_changes(), 1);
        assert_eq!(backend.to_text(), "  X  ");
    }
}
