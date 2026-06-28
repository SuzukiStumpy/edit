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
use crate::canvas::Canvas;
use crate::command::{CM_QUIT, CommandSet};
use crate::event::{Event, EventResult};
use crate::view::{Context, View};
use std::collections::VecDeque;
use std::io;
use std::time::Duration;

/// The default idle/blink cadence: how long [`Application::run`] waits for input
/// before synthesising an [`Event::Idle`].
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(100);

/// Cap on the number of posted events processed per input event. A hang-guard:
/// a misbehaving view that posts a command in response to handling one drops
/// events past the cap rather than spinning the loop forever.
const MAX_POSTED_PER_EVENT: usize = 1024;

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

/// The root of the view tree, adapted to the loop's [`Program`] contract.
///
/// `Root` owns the top-level [`View`] and the [`CommandSet`], and is where the
/// two halves of the command model meet (ADR 0003, 0004): an input event is
/// dispatched into the tree through a [`Context`]; whatever the tree *posts* in
/// response — commands bubbling up, broadcasts going down — is drained and
/// re-dispatched from the root until it settles. [`CM_QUIT`] is the one command
/// `Root` handles itself: it ends the loop. This replaces Phase 2's quit-flag
/// stepping stone with the real command path.
pub struct Root {
    view: Box<dyn View>,
    commands: CommandSet,
    finished: bool,
}

impl Root {
    /// Creates a root over `view`, with every command enabled.
    pub fn new(view: Box<dyn View>) -> Self {
        Self {
            view,
            commands: CommandSet::new(),
            finished: false,
        }
    }

    /// Starts from a given command-enable set (e.g. with some commands disabled).
    pub fn with_commands(mut self, commands: CommandSet) -> Self {
        self.commands = commands;
        self
    }

    /// The command-enable set.
    pub fn commands(&self) -> &CommandSet {
        &self.commands
    }

    /// The command-enable set, mutably — enable/disable as application state
    /// changes (a control reads this to grey itself).
    pub fn commands_mut(&mut self) -> &mut CommandSet {
        &mut self.commands
    }

    /// Delivers one event to the view tree, queueing whatever it posts.
    fn deliver(&mut self, event: &Event, queue: &mut VecDeque<Event>) -> EventResult {
        // Split-borrow the disjoint fields so the tree can be handled mutably
        // while the context reads the command set.
        let Self { view, commands, .. } = self;
        let mut ctx = Context::new(commands);
        let result = view.handle_event(event, &mut ctx);
        queue.extend(ctx.take_posted());
        result
    }

    /// Dispatches `event`, then drains posted commands/broadcasts, re-dispatching
    /// each from the root. [`CM_QUIT`] ends the loop; everything else flows back
    /// into the tree. Returns the result of the original event.
    fn dispatch(&mut self, event: &Event) -> EventResult {
        let mut queue = VecDeque::new();
        let result = self.deliver(event, &mut queue);
        let mut budget = MAX_POSTED_PER_EVENT;
        while let Some(posted) = queue.pop_front() {
            if posted == Event::Command(CM_QUIT) {
                self.finished = true;
                continue;
            }
            if budget == 0 {
                break;
            }
            budget -= 1;
            self.deliver(&posted, &mut queue);
        }
        result
    }
}

impl Program for Root {
    fn draw(&mut self, frame: &mut Buffer) {
        // The root view fills the terminal: hand it a canvas over the whole frame
        // so it can lay itself out against the live size (centre, reflow on
        // resize) via `Canvas::size`. Its own `bounds` is for nesting in an owner,
        // which the root has none of.
        let mut canvas = Canvas::new(frame);
        self.view.draw(&mut canvas);
    }

    fn handle_event(&mut self, event: &Event) -> EventResult {
        self.dispatch(event)
    }

    fn is_finished(&self) -> bool {
        self.finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::TestBackend;
    use crate::color::Style;
    use crate::command::{CM_USER, Command};
    use crate::event::{KeyCode, KeyEvent, Modifiers};
    use crate::geometry::{Point, Rect, Size};
    use crate::view::StaticText;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

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

    // --- Root: the view-tree bridge to the loop (Phase 3) ---

    /// A focusable leaf for driving `Root`: posts `command` when it sees `on_key`,
    /// and records every command it is handed (without consuming it, so a
    /// re-dispatched command bubbles back out).
    struct Poster {
        bounds: Rect,
        on_key: KeyCode,
        command: Command,
        received: Rc<RefCell<Vec<Command>>>,
    }

    impl View for Poster {
        fn bounds(&self) -> Rect {
            self.bounds
        }

        fn draw(&self, canvas: &mut Canvas) {
            canvas.put_str(Point::new(0, 0), "P", Style::new());
        }

        fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
            match event {
                Event::Key(key) if key.code == self.on_key => {
                    ctx.post(self.command);
                    EventResult::Consumed
                }
                Event::Command(command) => {
                    self.received.borrow_mut().push(*command);
                    EventResult::Ignored
                }
                _ => EventResult::Ignored,
            }
        }

        fn focusable(&self) -> bool {
            true
        }
    }

    fn full(size: Size) -> Rect {
        Rect::from_origin_size(Point::new(0, 0), size)
    }

    #[test]
    fn root_draws_its_view_through_a_canvas() {
        let mut root = Root::new(Box::new(StaticText::new(
            full(Size::new(8, 1)),
            "hello",
            Style::new(),
        )));
        let mut frame = Buffer::new(Size::new(8, 1));
        root.draw(&mut frame);
        assert_eq!(frame.to_text(), "hello   ");
    }

    #[test]
    fn posting_cm_quit_finishes_the_root() {
        // The view posts CM_QUIT on Ctrl-Q; Root drains it and ends the loop —
        // the real command path replacing Phase 2's quit flag.
        let mut root = Root::new(Box::new(Poster {
            bounds: full(Size::new(10, 3)),
            on_key: KeyCode::Char('q'),
            command: CM_QUIT,
            received: Rc::new(RefCell::new(Vec::new())),
        }));
        assert!(!root.is_finished());
        root.handle_event(&Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            Modifiers::NONE,
        )));
        assert!(root.is_finished());
    }

    #[test]
    fn a_posted_command_is_redispatched_into_the_tree() {
        // The view posts an app command on Enter; Root re-dispatches it from the
        // top, so the tree sees it come back as an Event::Command.
        let app_cmd = Command(CM_USER + 1);
        let received = Rc::new(RefCell::new(Vec::new()));
        let mut root = Root::new(Box::new(Poster {
            bounds: full(Size::new(10, 3)),
            on_key: KeyCode::Enter,
            command: app_cmd,
            received: Rc::clone(&received),
        }));

        root.handle_event(&Event::Key(KeyEvent::new(KeyCode::Enter, Modifiers::NONE)));
        assert_eq!(*received.borrow(), vec![app_cmd]);
        assert!(!root.is_finished(), "an app command must not end the loop");
    }

    #[test]
    fn a_disabled_command_is_neither_posted_nor_redispatched() {
        let app_cmd = Command(CM_USER + 2);
        let received = Rc::new(RefCell::new(Vec::new()));
        let mut commands = CommandSet::new();
        commands.disable(app_cmd);
        let mut root = Root::new(Box::new(Poster {
            bounds: full(Size::new(10, 3)),
            on_key: KeyCode::Enter,
            command: app_cmd,
            received: Rc::clone(&received),
        }))
        .with_commands(commands);

        root.handle_event(&Event::Key(KeyEvent::new(KeyCode::Enter, Modifiers::NONE)));
        assert!(
            received.borrow().is_empty(),
            "a disabled command never fires, so nothing is re-dispatched"
        );
    }

    #[test]
    fn application_runs_the_root_until_a_view_posts_cm_quit() {
        // End-to-end through the real loop: a key reaches the focused view, which
        // posts CM_QUIT; Root finishes; the loop exits after presenting.
        let quit_key = Event::Key(KeyEvent::new(KeyCode::Char('q'), Modifiers::CONTROL));
        let terminal = ScriptedTerminal::new(Size::new(8, 1), vec![Some(quit_key)]);
        let mut app = Application::new(terminal);
        let mut root = Root::new(Box::new(Poster {
            bounds: full(Size::new(8, 1)),
            on_key: KeyCode::Char('q'),
            command: CM_QUIT,
            received: Rc::new(RefCell::new(Vec::new())),
        }));

        app.run(&mut root).unwrap();

        assert!(root.is_finished());
        assert!(app.terminal().presents() >= 1);
        assert!(app.terminal().screen_text().starts_with('P'));
    }
}
