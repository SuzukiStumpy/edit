//! The application driver: owns the terminal seam and runs the main loop.
//!
//! Each turn the loop builds a fresh frame, lets the [`Program`] draw into it,
//! presents it (a minimal diff flush — ADR 0002), then waits up to a timeout for
//! the next event and hands it to the program. A timed-out wait becomes
//! [`Event::Idle`], so the timeout is the idle/blink cadence.
//!
//! The thing the loop drives is abstracted behind [`Program`] so the loop is
//! unit-testable against a scripted, headless terminal with no real TTY. In
//! Phase 3 the root view tree takes the [`Program`] role; in Phase 2 a demo or a
//! test does (ADR 0003, 0004).

use crate::backend::{Backend, EventSource};
use crate::buffer::Buffer;
use crate::event::{Event, EventResult};
use std::io;
use std::time::Duration;

/// The default idle/blink cadence: how long [`Application::run`] waits for input
/// before synthesising an [`Event::Idle`].
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(100);

/// What the [`Application`] loop drives: something that can render itself, react
/// to events, and say when it is done. The Phase 3 view tree will implement this.
pub trait Program {
    /// Renders the current state into `frame` (a blank buffer of the terminal's
    /// current size).
    fn draw(&mut self, frame: &mut Buffer);

    /// Reacts to one event, returning whether it was consumed (ADR 0004).
    fn handle_event(&mut self, event: &Event) -> EventResult;

    /// Returns whether the loop should stop. Checked after each draw and after
    /// each handled event.
    fn is_finished(&self) -> bool;
}

/// Owns the terminal (a combined [`Backend`] + [`EventSource`]) and runs the loop.
///
/// Because the `Application` owns the terminal, any unwind through [`run`](Self::run)
/// drops it — and the real backend's `Drop` restores the terminal (ADR 0001).
pub struct Application<T> {
    terminal: T,
    timeout: Duration,
}

impl<T: Backend + EventSource> Application<T> {
    /// Creates an application over `terminal` with the default idle cadence.
    pub fn new(terminal: T) -> Self {
        Self {
            terminal,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Sets the idle/blink cadence (the poll timeout).
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The current idle/blink cadence.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Borrows the terminal (e.g. to inspect a test backend's screen).
    pub fn terminal(&self) -> &T {
        &self.terminal
    }

    /// Mutably borrows the terminal.
    pub fn terminal_mut(&mut self) -> &mut T {
        &mut self.terminal
    }

    /// Runs the loop until `program` reports it is finished.
    ///
    /// Each turn: build a frame at the terminal's current size, `draw`, `present`,
    /// stop if finished, else wait for an event (`None` ⇒ [`Event::Idle`]),
    /// handle it, stop if finished. The two finish checks bracket the wait so a
    /// program that finishes while handling an event exits without a spurious
    /// extra draw, while one that starts finished still paints once.
    ///
    /// # Errors
    ///
    /// Propagates any I/O error from presenting a frame or polling for events.
    pub fn run(&mut self, program: &mut impl Program) -> io::Result<()> {
        loop {
            let mut frame = Buffer::new(self.terminal.size());
            program.draw(&mut frame);
            self.terminal.present(&frame)?;
            if program.is_finished() {
                break;
            }

            let event = self
                .terminal
                .poll_event(self.timeout)?
                .unwrap_or(Event::Idle);
            program.handle_event(&event);
            if program.is_finished() {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::TestBackend;
    use crate::color::Style;
    use crate::event::{KeyCode, KeyEvent, Modifiers};
    use crate::geometry::{Point, Size};
    use std::collections::VecDeque;

    /// A headless terminal for driving the loop: delegates output to a
    /// [`TestBackend`] and replays a script of poll results. Each `poll_event`
    /// pops the next scripted result; a `Resize` updates the reported size, the
    /// way the real backend does. The script running dry is an error rather than a
    /// silent idle, so a loop that fails to finish surfaces as a failed test
    /// instead of a hang.
    struct ScriptedTerminal {
        backend: TestBackend,
        size: Size,
        script: VecDeque<Option<Event>>,
    }

    impl ScriptedTerminal {
        fn new(size: Size, script: Vec<Option<Event>>) -> Self {
            Self {
                backend: TestBackend::new(size),
                size,
                script: script.into_iter().collect(),
            }
        }

        fn screen_text(&self) -> String {
            self.backend.to_text()
        }

        fn presents(&self) -> usize {
            self.backend.presents()
        }
    }

    impl Backend for ScriptedTerminal {
        fn size(&self) -> Size {
            self.size
        }

        fn present(&mut self, frame: &Buffer) -> io::Result<()> {
            self.backend.present(frame)
        }
    }

    impl EventSource for ScriptedTerminal {
        fn poll_event(&mut self, _timeout: Duration) -> io::Result<Option<Event>> {
            match self.script.pop_front() {
                Some(result) => {
                    if let Some(Event::Resize(size)) = result {
                        self.size = size;
                    }
                    Ok(result)
                }
                None => Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "scripted terminal ran out of events before the program finished",
                )),
            }
        }
    }

    /// A program that records what it sees, paints `HI`, and quits on Ctrl-Q.
    #[derive(Default)]
    struct Recorder {
        seen: Vec<Event>,
        draw_sizes: Vec<Size>,
        finished: bool,
    }

    impl Program for Recorder {
        fn draw(&mut self, frame: &mut Buffer) {
            self.draw_sizes.push(frame.size());
            frame.put_str(Point::new(0, 0), "HI", Style::new());
        }

        fn handle_event(&mut self, event: &Event) -> EventResult {
            self.seen.push(*event);
            if let Event::Key(KeyEvent {
                code: KeyCode::Char('q'),
                modifiers,
            }) = event
            {
                if modifiers.contains(Modifiers::CONTROL) {
                    self.finished = true;
                    return EventResult::Consumed;
                }
            }
            EventResult::Ignored
        }

        fn is_finished(&self) -> bool {
            self.finished
        }
    }

    fn ctrl_q() -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char('q'), Modifiers::CONTROL))
    }

    #[test]
    fn draws_then_quits_on_the_quit_key() {
        let terminal = ScriptedTerminal::new(Size::new(6, 2), vec![Some(ctrl_q())]);
        let mut app = Application::new(terminal);
        let mut program = Recorder::default();

        app.run(&mut program).unwrap();

        assert!(program.finished);
        assert_eq!(program.seen, vec![ctrl_q()]);
        // The program's drawing reached the screen, and we presented at least once.
        assert!(app.terminal().screen_text().starts_with("HI"));
        assert!(app.terminal().presents() >= 1);
    }

    #[test]
    fn a_timed_out_poll_delivers_one_idle() {
        // `None` is a poll timeout; the loop turns it into exactly one Idle.
        let terminal = ScriptedTerminal::new(Size::new(6, 2), vec![None, Some(ctrl_q())]);
        let mut app = Application::new(terminal);
        let mut program = Recorder::default();

        app.run(&mut program).unwrap();

        assert_eq!(program.seen, vec![Event::Idle, ctrl_q()]);
    }

    #[test]
    fn a_resize_changes_the_next_draw_size() {
        let terminal = ScriptedTerminal::new(
            Size::new(6, 2),
            vec![Some(Event::Resize(Size::new(10, 3))), Some(ctrl_q())],
        );
        let mut app = Application::new(terminal);
        let mut program = Recorder::default();

        app.run(&mut program).unwrap();

        // First draw at the initial size, then at the resized size.
        assert_eq!(program.draw_sizes, vec![Size::new(6, 2), Size::new(10, 3)]);
        assert_eq!(program.seen[0], Event::Resize(Size::new(10, 3)));
    }
}
