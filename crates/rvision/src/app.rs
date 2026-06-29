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
use crate::command::{CM_QUIT, Command, CommandSet};
use crate::event::{Event, EventResult, MouseEvent};
use crate::geometry::{Point, Rect, Size};
use crate::view::{Context, Modal, View};
use crate::widgets::{Desktop, MenuBar, StatusLine};
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

    /// Runs `dialog` modally over `background`, returning the command that closed
    /// it (ADR 0017) — TurboVision's `execView`.
    ///
    /// Each turn: build a frame at the terminal's current size, let `background`
    /// **draw** (it receives no events while the dialog is up), centre the dialog
    /// and draw it on top, present, then poll one event and hand it to the dialog.
    /// A positional event is translated into the dialog's local coordinates. The
    /// first *ending* command the dialog posts ([`Modal::ends_on`]) returns from
    /// the loop; any other posted command/broadcast is re-dispatched into the
    /// dialog, exactly as [`Root`] drains the tree. `Esc` closes it as `CM_CANCEL`.
    ///
    /// The dialog never joins the application's view tree; it is the caller's,
    /// borrowed for the duration of the loop and untouched afterwards.
    ///
    /// # Errors
    ///
    /// Propagates any I/O error from presenting a frame or polling for events.
    pub fn exec_view(
        &mut self,
        background: &mut dyn Program,
        dialog: &mut dyn Modal,
    ) -> io::Result<Command> {
        let commands = CommandSet::new();
        loop {
            let size = self.terminal.size();
            let mut frame = Buffer::new(size);
            background.draw(&mut frame);

            let area = centered(dialog.size(), size);
            {
                let mut canvas = Canvas::new(&mut frame);
                // The modal casts its own drop shadow on the background it floats
                // over, through the per-view protocol (ADR 0020).
                if let Some(style) = dialog.drop_shadow() {
                    canvas.shadow(area, style);
                }
                let mut sub = canvas.child(area);
                dialog.draw(&mut sub);
            }
            self.terminal.present(&frame)?;

            let event = self
                .terminal
                .poll_event(self.timeout)?
                .unwrap_or(Event::Idle);
            // Translate a positional event into the dialog's local coordinates;
            // everything else passes through unchanged.
            let event = match event {
                Event::Mouse(mouse) => Event::Mouse(MouseEvent {
                    pos: mouse.pos.offset(-area.origin().x, -area.origin().y),
                    ..mouse
                }),
                other => other,
            };

            if let Some(command) = dispatch_modal(dialog, &event, &commands) {
                return Ok(command);
            }
        }
    }
}

/// Centres a box of `size` within `within`, clamped to fit.
fn centered(size: Size, within: Size) -> Rect {
    let w = size.width.clamp(0, within.width.max(0));
    let h = size.height.clamp(0, within.height.max(0));
    let x = ((within.width - w) / 2).max(0);
    let y = ((within.height - h) / 2).max(0);
    Rect::from_origin_size(Point::new(x, y), Size::new(w, h))
}

/// Delivers one event to `dialog` and drains what it posts: a posted *ending*
/// command (`Dialog::ends_on`) is returned to stop the modal loop; any other
/// posted command/broadcast is re-dispatched into the dialog. Returns `None` if
/// nothing ended the loop.
fn dispatch_modal(dialog: &mut dyn Modal, event: &Event, commands: &CommandSet) -> Option<Command> {
    let mut queue = VecDeque::new();
    {
        let mut ctx = Context::new(commands);
        dialog.handle_event(event, &mut ctx);
        queue.extend(ctx.take_posted());
    }
    let mut budget = MAX_POSTED_PER_EVENT;
    while let Some(posted) = queue.pop_front() {
        if let Event::Command(command) = posted {
            if dialog.ends_on(command) {
                return Some(command);
            }
        }
        if budget == 0 {
            break;
        }
        budget -= 1;
        let mut ctx = Context::new(commands);
        dialog.handle_event(&posted, &mut ctx);
        queue.extend(ctx.take_posted());
    }
    None
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

/// The standard application screen: a menu bar across the top, a status line
/// across the bottom, and a desktop filling the space between — TurboVision's
/// `TProgram` (ADR 0016).
///
/// `Shell` is itself a [`View`], so it drops into a [`Root`] and runs in the
/// [`Application`] loop. Unlike a generic [`Group`](crate::view::Group) it:
///
/// - **lays out live**: each frame it carves the three regions from the terminal
///   size, so a resize relays them out (windows inside the desktop keep their
///   place);
/// - **draws the menu overlay last**: an open pull-down paints over the whole
///   frame, on top of everything (the menu bar's own row is only one tall);
/// - **routes keys in three local passes** — menu bar (pre-process: `Alt`-hot-keys
///   and, while open, modally), then the desktop's active window (focused), then
///   the status line (post-process: global function keys). The generic event
///   engine is untouched; these passes live only here, where the only views that
///   need them are.
pub struct Shell {
    menu_bar: MenuBar,
    desktop: Desktop,
    status_line: StatusLine,
    size: Size,
}

/// The three chrome regions for a terminal of `size`: the menu-bar row, the
/// desktop between, and the status-line row.
struct Regions {
    menu: Rect,
    desktop: Rect,
    status: Rect,
}

fn regions(size: Size) -> Regions {
    let w = size.width.max(0);
    let h = size.height.max(0);
    Regions {
        menu: Rect::from_origin_size(Point::new(0, 0), Size::new(w, 1)),
        desktop: Rect::from_origin_size(Point::new(0, 1), Size::new(w, (h - 2).max(0))),
        status: Rect::from_origin_size(Point::new(0, (h - 1).max(0)), Size::new(w, 1)),
    }
}

impl Shell {
    /// Assembles a shell for a terminal of `size` from its three chrome pieces,
    /// positioning each to the matching region.
    pub fn new(size: Size, menu_bar: MenuBar, desktop: Desktop, status_line: StatusLine) -> Self {
        let mut shell = Self {
            menu_bar,
            desktop,
            status_line,
            size,
        };
        shell.relayout(size);
        shell
    }

    /// Whether a menu pull-down is currently open (the menu bar runs modally then).
    pub fn menu_is_open(&self) -> bool {
        self.menu_bar.is_open()
    }

    /// Repositions the three children for a terminal of `size`.
    fn relayout(&mut self, size: Size) {
        self.size = size;
        let r = regions(size);
        self.menu_bar.set_bounds(r.menu);
        self.desktop.set_bounds(r.desktop);
        self.status_line.set_bounds(r.status);
    }

    /// Three-pass key routing (ADR 0016): menu bar → active window → status line.
    fn handle_key(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        self.menu_bar
            .handle_event(event, ctx)
            .or_else(|| self.desktop.handle_event(event, ctx))
            .or_else(|| self.status_line.handle_event(event, ctx))
    }

    /// Positional routing: the region under the pointer, in that region's local
    /// coordinates. (Behaviour inside each region is mostly Phase 9; the seam is
    /// here from the start — ADR 0007.)
    fn handle_mouse(&mut self, mouse: MouseEvent, ctx: &mut Context) -> EventResult {
        let r = regions(self.size);
        for (region, target) in [
            (r.menu, &mut self.menu_bar as &mut dyn View),
            (r.status, &mut self.status_line as &mut dyn View),
            (r.desktop, &mut self.desktop as &mut dyn View),
        ] {
            if region.contains(mouse.pos) {
                let local = MouseEvent {
                    pos: mouse.pos.offset(-region.origin().x, -region.origin().y),
                    ..mouse
                };
                return target.handle_event(&Event::Mouse(local), ctx);
            }
        }
        EventResult::Ignored
    }
}

impl View for Shell {
    fn bounds(&self) -> Rect {
        Rect::from_origin_size(Point::new(0, 0), self.size)
    }

    fn draw(&self, canvas: &mut Canvas) {
        let r = regions(canvas.size());
        // Each child draws through a sub-canvas scoped so its borrow ends before
        // the next one reborrows the frame.
        self.desktop.draw(&mut canvas.child(r.desktop));
        self.status_line.draw(&mut canvas.child(r.status));
        self.menu_bar.draw(&mut canvas.child(r.menu));
        // The open pull-down is the last thing drawn, over the whole frame, so it
        // sits on top of the desktop below the bar (ADR 0016).
        self.menu_bar.draw_overlay(canvas);
    }

    fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        match event {
            Event::Key(_) => self.handle_key(event, ctx),
            Event::Mouse(mouse) => self.handle_mouse(*mouse, ctx),
            // A re-dispatched command (ADR 0003) goes to the active window; CM_QUIT
            // never reaches here — `Root` claims it before re-dispatch.
            Event::Command(_) => self.desktop.handle_event(event, ctx),
            Event::Resize(size) => {
                self.relayout(*size);
                self.desktop.handle_event(event, ctx);
                EventResult::Ignored
            }
            Event::Broadcast(_) | Event::Idle => {
                self.menu_bar.handle_event(event, ctx);
                self.desktop.handle_event(event, ctx);
                self.status_line.handle_event(event, ctx);
                EventResult::Ignored
            }
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::TestBackend;
    use crate::cell::Cell;
    use crate::color::Style;
    use crate::command::{CM_CANCEL, CM_OK, CM_USER, Command};
    use crate::event::{KeyCode, KeyEvent, Modifiers};
    use crate::geometry::{Point, Rect, Size};
    use crate::theme::{Role, Theme};
    use crate::view::StaticText;
    use crate::widgets::{Menu, MenuItem, StatusItem, Window};
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

        fn screen(&self) -> &Buffer {
            self.backend.screen()
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

    // --- Shell: the TProgram-style application root (Phase 4) ---

    const CM_PING: Command = Command(CM_USER + 10);
    const CM_HELP: Command = Command(CM_USER + 11);

    /// Builds a shell with one window whose interior posts [`CM_PING`] on `a`, a
    /// File/Edit menu bar, and an F1/Alt-X status line.
    fn shell(size: Size, received: Rc<RefCell<Vec<Command>>>) -> Shell {
        use crate::theme::Role;
        let theme = Theme::default();
        let (w, h) = (size.width, size.height);

        let menu_bar = MenuBar::new(
            full(Size::new(w, 1)),
            vec![
                Menu::new("File", vec![MenuItem::new("Exit", CM_QUIT)]),
                Menu::new("Edit", vec![MenuItem::new("Copy", Command(CM_USER + 20))]),
            ],
            &theme,
        );
        let window = Window::new(
            Rect::from_origin_size(Point::new(2, 1), Size::new(30, 6)),
            "Untitled",
            &theme,
            Box::new(Poster {
                bounds: full(Size::new(28, 4)),
                on_key: KeyCode::Char('a'),
                command: CM_PING,
                received,
            }),
        );
        let desktop = Desktop::new(
            Rect::from_origin_size(Point::new(0, 1), Size::new(w, h - 2)),
            Cell::from_char('░', theme.style(Role::DesktopBackground)),
            vec![window],
        );
        let status = StatusLine::new(
            Rect::from_origin_size(Point::new(0, h - 1), Size::new(w, 1)),
            vec![
                StatusItem::new(
                    "F1",
                    "Help",
                    KeyEvent::new(KeyCode::F(1), Modifiers::NONE),
                    CM_HELP,
                ),
                StatusItem::new(
                    "Alt-X",
                    "Exit",
                    KeyEvent::new(KeyCode::Char('x'), Modifiers::ALT),
                    CM_QUIT,
                ),
            ],
            theme.style(Role::StatusBar),
            theme.style(Role::StatusKey),
        );
        Shell::new(size, menu_bar, desktop, status)
    }

    fn no_log() -> Rc<RefCell<Vec<Command>>> {
        Rc::new(RefCell::new(Vec::new()))
    }

    #[test]
    fn snapshot_shell_composes_the_full_screen() {
        let sh = shell(Size::new(40, 10), no_log());
        let mut frame = Buffer::new(Size::new(40, 10));
        let mut canvas = Canvas::new(&mut frame);
        sh.draw(&mut canvas);
        insta::assert_snapshot!(frame.to_text());
    }

    #[test]
    fn a_plain_key_reaches_the_active_window() {
        let mut sh = shell(Size::new(40, 10), no_log());
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        // Closed menu: 'a' flows past the bar to the active window, which posts ping.
        sh.handle_event(
            &Event::Key(KeyEvent::new(KeyCode::Char('a'), Modifiers::NONE)),
            &mut ctx,
        );
        assert_eq!(ctx.posted(), &[Event::Command(CM_PING)]);
    }

    #[test]
    fn an_open_menu_swallows_typing_so_the_window_never_sees_it() {
        let mut sh = shell(Size::new(40, 10), no_log());
        let cs = CommandSet::new();
        sh.handle_event(
            &Event::Key(KeyEvent::new(KeyCode::Char('f'), Modifiers::ALT)),
            &mut Context::new(&cs),
        );
        assert!(sh.menu_is_open());
        // 'a' now goes to the modal menu, not the window: no ping is posted.
        let mut ctx = Context::new(&cs);
        sh.handle_event(
            &Event::Key(KeyEvent::new(KeyCode::Char('a'), Modifiers::NONE)),
            &mut ctx,
        );
        assert!(ctx.posted().is_empty());
        assert!(sh.menu_is_open());
    }

    #[test]
    fn a_function_key_falls_through_to_the_status_line() {
        let mut sh = shell(Size::new(40, 10), no_log());
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        // The window's interior ignores F1, so the post-process status line claims it.
        sh.handle_event(
            &Event::Key(KeyEvent::new(KeyCode::F(1), Modifiers::NONE)),
            &mut ctx,
        );
        assert_eq!(ctx.posted(), &[Event::Command(CM_HELP)]);
    }

    #[test]
    fn resize_relays_out_the_chrome() {
        let mut sh = shell(Size::new(20, 6), no_log());
        sh.handle_event(
            &Event::Resize(Size::new(30, 8)),
            &mut Context::new(&CommandSet::new()),
        );
        assert_eq!(sh.size, Size::new(30, 8));
        // Drawn at the new size, the status line lands on the new bottom row (7).
        let mut frame = Buffer::new(Size::new(30, 8));
        let mut canvas = Canvas::new(&mut frame);
        sh.draw(&mut canvas);
        let text = frame.to_text();
        let rows: Vec<&str> = text.lines().collect();
        assert!(rows[7].trim_start().starts_with("F1 Help"));
    }

    #[test]
    fn application_runs_the_shell_until_a_status_key_posts_cm_quit() {
        // End-to-end: Alt-X reaches the status line (the window ignores it), which
        // posts CM_QUIT; Root claims it and the loop exits.
        let alt_x = Event::Key(KeyEvent::new(KeyCode::Char('x'), Modifiers::ALT));
        let terminal = ScriptedTerminal::new(Size::new(40, 10), vec![Some(alt_x)]);
        let mut app = Application::new(terminal);
        let mut root = Root::new(Box::new(shell(Size::new(40, 10), no_log())));

        app.run(&mut root).unwrap();

        assert!(root.is_finished());
        assert!(app.terminal().presents() >= 1);
    }

    // --- exec_view: the modal dialog loop (Phase 5, ADR 0017) ---

    /// A background program that just paints `BG` at the origin (outside any
    /// centred dialog), so a test can confirm the background was drawn underneath.
    #[derive(Default)]
    struct Backdrop;

    impl Program for Backdrop {
        fn draw(&mut self, frame: &mut Buffer) {
            frame.put_str(Point::new(0, 0), "BG", Style::new());
        }
        fn handle_event(&mut self, _event: &Event) -> EventResult {
            EventResult::Ignored
        }
        fn is_finished(&self) -> bool {
            false
        }
    }

    fn message_box() -> crate::widgets::Dialog {
        crate::widgets::MessageBox::ok_cancel("Confirm", "Proceed?", &Theme::default())
    }

    #[test]
    fn exec_view_returns_ok_when_the_default_button_is_pressed() {
        // Focus starts on the OK button; Enter activates it and ends the loop.
        let enter = Event::Key(KeyEvent::new(KeyCode::Enter, Modifiers::NONE));
        let terminal = ScriptedTerminal::new(Size::new(40, 12), vec![Some(enter)]);
        let mut app = Application::new(terminal);
        let mut background = Backdrop;
        let mut dialog = message_box();

        let result = app.exec_view(&mut background, &mut dialog).unwrap();
        assert_eq!(result, CM_OK);
        // The background was painted under the centred dialog, and we presented.
        assert!(app.terminal().screen_text().starts_with("BG"));
        assert!(app.terminal().presents() >= 1);
    }

    #[test]
    fn exec_view_casts_a_drop_shadow_under_the_dialog() {
        let enter = Event::Key(KeyEvent::new(KeyCode::Enter, Modifiers::NONE));
        let size = Size::new(40, 12);
        let terminal = ScriptedTerminal::new(size, vec![Some(enter)]);
        let mut app = Application::new(terminal);
        let mut background = Backdrop;
        let mut dialog = message_box();
        let area = centered(dialog.size(), size);

        app.exec_view(&mut background, &mut dialog).unwrap();

        // A cell just past the dialog's right edge, one row below its top, is dimmed
        // to the shadow style — the dialog floats over the background.
        let screen = app.terminal().screen();
        let shadow_cell = screen
            .get(Point::new(area.bottom_right().x, area.origin().y + 1))
            .unwrap();
        assert_eq!(shadow_cell.style(), Theme::default().style(Role::Shadow));
    }

    #[test]
    fn exec_view_returns_cancel_on_esc() {
        let esc = Event::Key(KeyEvent::new(KeyCode::Esc, Modifiers::NONE));
        let terminal = ScriptedTerminal::new(Size::new(40, 12), vec![Some(esc)]);
        let mut app = Application::new(terminal);
        let mut background = Backdrop;
        let mut dialog = message_box();

        let result = app.exec_view(&mut background, &mut dialog).unwrap();
        assert_eq!(result, CM_CANCEL);
    }

    #[test]
    fn exec_view_idles_until_a_button_is_pressed() {
        // A timed-out poll (None ⇒ Idle) keeps the loop alive without ending it;
        // Tab then Enter selects Cancel.
        let tab = Event::Key(KeyEvent::new(KeyCode::Tab, Modifiers::NONE));
        let enter = Event::Key(KeyEvent::new(KeyCode::Enter, Modifiers::NONE));
        let terminal = ScriptedTerminal::new(Size::new(40, 12), vec![None, Some(tab), Some(enter)]);
        let mut app = Application::new(terminal);
        let mut background = Backdrop;
        let mut dialog = message_box();

        let result = app.exec_view(&mut background, &mut dialog).unwrap();
        assert_eq!(
            result, CM_CANCEL,
            "Tab moved focus to Cancel, Enter chose it"
        );
    }
}
