//! The same screen as the `hello` demo, rebuilt on the Phase 3 view system to
//! show how the plumbing changed.
//!
//! Run with `cargo run -p rvision --example hello2`. It looks and behaves like
//! `hello` — a centred window, an idle spinner, quit on Ctrl-Q/Esc, panic-safe
//! restore — but where `hello` is one `Program` with a single monolithic `draw`
//! and a `finished` flag, this is:
//!
//! - a **retained view tree**: a `Desktop` view owning a `Group` window, whose
//!   body is `StaticText` + a `Spinner`, each drawing through its **own** clipped,
//!   offset `Canvas` in local coordinates (ADR 0015) — no view computes a screen
//!   position;
//! - **commands, not flags**: a quit key posts `CM_QUIT` to the `Context`; the
//!   `Root` drains it and ends the loop (ADR 0003, 0004);
//! - driven by **`Root`**, the bridge from the view tree to the Phase 2 loop.
//!
//! Layout note: dynamic centring/reflow is done here *by the example* (the
//! `Desktop` reads its live `Canvas` size each frame). Phase 4 folds that into a
//! reusable `Desktop` view; for now the framework just gives us the seam.

use std::io;
use std::time::Duration;

use rvision::app::{Application, Root};
use rvision::canvas::Canvas;
use rvision::cell::Cell;
use rvision::color::Style;
use rvision::command::CM_QUIT;
use rvision::crossterm_backend::CrosstermBackend;
use rvision::event::{Event, EventResult, KeyCode, Modifiers};
use rvision::geometry::{Point, Rect, Size};
use rvision::theme::{Role, Theme};
use rvision::view::{Context, Group, StaticText, View};

/// The spinner glyphs cycled on each idle tick, to prove the timeout path works.
const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

/// The window's fixed interior size; the `Desktop` positions it, the window lays
/// its children out relative to its own top-left.
const WINDOW_SIZE: Size = Size::new(46, 9);

/// The window chrome: a filled panel, a single-line border, and a title on the
/// top edge. A leaf view drawn underneath the body text (z-order: first child).
struct Frame {
    bounds: Rect,
    panel: Style,
    title: Style,
    label: String,
}

impl View for Frame {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn draw(&self, canvas: &mut Canvas) {
        let area = canvas.bounds();
        canvas.fill(area, &Cell::blank(self.panel));
        canvas.draw_box(area, self.panel);
        canvas.put_str(Point::new(2, 0), &self.label, self.title);
    }
}

/// A status line that advances a spinner glyph on every `Event::Idle` it is
/// handed (broadcast down the tree), proving the idle cadence still drives work.
struct Spinner {
    bounds: Rect,
    style: Style,
    ticks: u64,
}

impl View for Spinner {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn draw(&self, canvas: &mut Canvas) {
        let glyph = SPINNER[(self.ticks % 4) as usize];
        canvas.put_str(
            Point::new(0, 0),
            &format!("Group · StaticText · Canvas      idle {glyph}"),
            self.style,
        );
    }

    fn handle_event(&mut self, event: &Event, _ctx: &mut Context) -> EventResult {
        if let Event::Idle = event {
            self.ticks += 1;
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }
}

/// Builds the window: a `Group` whose children are laid out in coordinates
/// relative to the window's own top-left (ADR 0015). The `Group` draws them in
/// order, each through its own clipped sub-`Canvas`.
fn window(theme: &Theme) -> Group {
    let panel = theme.style(Role::WindowFrame);
    let title = theme.style(Role::WindowTitle);
    let hint = theme.style(Role::StatusKey);

    let full = Rect::from_origin_size(Point::new(0, 0), WINDOW_SIZE);
    let line = |row: i16| Rect::from_origin_size(Point::new(2, row), Size::new(42, 1));

    let children: Vec<Box<dyn View>> = vec![
        Box::new(Frame {
            bounds: full,
            panel,
            title,
            label: " rvision · hello2 ".to_string(),
        }),
        Box::new(StaticText::new(
            line(2),
            "The Phase 3 view tree is live.",
            panel,
        )),
        Box::new(Spinner {
            bounds: line(4),
            style: panel,
            ticks: 0,
        }),
        Box::new(StaticText::new(
            line(6),
            "Press Ctrl-Q or Esc to quit",
            hint,
        )),
    ];

    Group::new(full, children)
}

/// The top-level view: paints the desktop backdrop, centres the window against
/// the live screen size, posts `CM_QUIT` on a quit key, and forwards idle ticks
/// down to the window so the spinner advances.
struct Desktop {
    theme: Theme,
    window: Group,
}

impl Desktop {
    fn new() -> Self {
        let theme = Theme::default();
        let window = window(&theme);
        Self { theme, window }
    }
}

impl View for Desktop {
    fn bounds(&self) -> Rect {
        // Unused: `Root` draws the desktop across the whole frame (it has no
        // owner to be positioned within).
        Rect::from_origin_size(Point::new(0, 0), Size::new(0, 0))
    }

    fn draw(&self, canvas: &mut Canvas) {
        let area = canvas.bounds();
        canvas.fill(
            area,
            &Cell::blank(self.theme.style(Role::DesktopBackground)),
        );

        // Centre the window against the live terminal size.
        let screen = canvas.size();
        let w = WINDOW_SIZE.width.min(screen.width);
        let h = WINDOW_SIZE.height.min(screen.height);
        if w < 4 || h < 3 {
            return; // too small to bother
        }
        let origin = Point::new((screen.width - w) / 2, (screen.height - h) / 2);
        let mut sub = canvas.child(Rect::from_origin_size(origin, Size::new(w, h)));
        self.window.draw(&mut sub);
    }

    fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        match event {
            // Uncomment to prove panic-safe restore:
            // Event::Key(key) if matches!(key.code, KeyCode::Char('p')) => panic!("boom"),
            Event::Key(key) => {
                let quit = matches!(key.code, KeyCode::Esc)
                    || (matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q'))
                        && key.modifiers.contains(Modifiers::CONTROL));
                if quit {
                    // The command path, not a flag: Root turns CM_QUIT into a stop.
                    ctx.post(CM_QUIT);
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            // Let the spinner (down in the window) see the idle tick.
            Event::Idle => self.window.handle_event(event, ctx),
            _ => EventResult::Ignored,
        }
    }
}

fn main() -> io::Result<()> {
    let backend = CrosstermBackend::new()?;
    let mut app = Application::new(backend).with_timeout(Duration::from_millis(250));
    let mut root = Root::new(Box::new(Desktop::new()));
    app.run(&mut root)
    // `app` (and thus the backend) drops here, restoring the terminal.
}
