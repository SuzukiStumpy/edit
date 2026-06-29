//! The desktop: a backdrop with windows stacked on top of it (TurboVision's
//! `TDesktop`).
//!
//! It owns its [`Window`]s concretely — not as `Box<dyn View>` — so it can mark
//! the active (top) one, switching its frame to the doubled border. Windows are
//! stored bottom-to-top: index 0 draws first, the last draws on top and is the
//! active window. The desktop fills its whole area with the backdrop first, so the
//! gaps between and around windows always show the blue field.

use crate::canvas::Canvas;
use crate::cell::Cell;
use crate::event::{Event, EventResult, MouseEvent};
use crate::geometry::Rect;
use crate::view::{Context, View};

use super::Window;

/// A backdrop plus a stack of windows.
pub struct Desktop {
    bounds: Rect,
    backdrop: Cell,
    windows: Vec<Window>,
    active: Option<usize>,
}

impl Desktop {
    /// Creates a desktop occupying `bounds`, filled with `backdrop`, owning
    /// `windows` (index 0 at the bottom). The topmost window starts active.
    pub fn new(bounds: Rect, backdrop: Cell, mut windows: Vec<Window>) -> Self {
        let active = windows.len().checked_sub(1);
        if let Some(top) = active {
            windows[top].set_active(true);
        }
        Self {
            bounds,
            backdrop,
            windows,
            active,
        }
    }

    /// The index of the active (top) window, or `None` when the desktop is empty.
    pub fn active(&self) -> Option<usize> {
        self.active
    }

    /// Repositions the desktop (the shell calls this as the terminal resizes).
    pub fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
}

impl View for Desktop {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn draw(&self, canvas: &mut Canvas) {
        let area = canvas.bounds();
        canvas.fill(area, &self.backdrop);
        for window in &self.windows {
            // The window casts its drop shadow on the backdrop (or a lower
            // window) before it is drawn on top of that shadow (ADR 0020).
            if let Some(style) = window.drop_shadow() {
                canvas.shadow(window.bounds(), style);
            }
            let mut sub = canvas.child(window.bounds());
            window.draw(&mut sub);
        }
    }

    fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        match event {
            // Positional: the topmost window under the pointer, in its local coords.
            Event::Mouse(mouse) => {
                for window in self.windows.iter_mut().rev() {
                    let bounds = window.bounds();
                    if bounds.contains(mouse.pos) {
                        let local = MouseEvent {
                            pos: mouse.pos.offset(-bounds.origin().x, -bounds.origin().y),
                            ..*mouse
                        };
                        return window.handle_event(&Event::Mouse(local), ctx);
                    }
                }
                EventResult::Ignored
            }
            // Focused: only the active window. Its ignored result bubbles up so the
            // shell can try the status line next (ADR 0016). Paste rides along.
            Event::Key(_) | Event::Command(_) | Event::Paste(_) => match self.active {
                Some(index) => self.windows[index].handle_event(event, ctx),
                None => EventResult::Ignored,
            },
            // Broadcast / resize / idle: every window.
            Event::Broadcast(_) | Event::Resize(_) | Event::Idle => {
                for window in &mut self.windows {
                    window.handle_event(event, ctx);
                }
                EventResult::Ignored
            }
        }
    }

    fn focusable(&self) -> bool {
        self.active.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::color::Style;
    use crate::command::{CM_OK, Command, CommandSet};
    use crate::event::{KeyCode, KeyEvent, Modifiers, MouseButton, MouseKind};
    use crate::geometry::{Point, Size};
    use crate::theme::{Role, Theme};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn rect(x: i16, y: i16, w: i16, h: i16) -> Rect {
        Rect::from_origin_size(Point::new(x, y), Size::new(w, h))
    }

    /// An interior that records every event it sees and posts a command on Enter.
    struct Recorder {
        tag: u16,
        log: Rc<RefCell<Vec<(u16, Event)>>>,
        command: Command,
    }

    impl View for Recorder {
        fn bounds(&self) -> Rect {
            rect(0, 0, 100, 100)
        }
        fn draw(&self, _canvas: &mut Canvas) {}
        fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
            self.log.borrow_mut().push((self.tag, event.clone()));
            if matches!(event, Event::Key(k) if k.code == KeyCode::Enter) {
                ctx.post(self.command);
                return EventResult::Consumed;
            }
            EventResult::Ignored
        }
    }

    fn window(tag: u16, bounds: Rect, log: &Rc<RefCell<Vec<(u16, Event)>>>) -> Window {
        Window::new(
            bounds,
            "W",
            &Theme::default(),
            Box::new(Recorder {
                tag,
                log: Rc::clone(log),
                command: CM_OK,
            }),
        )
    }

    #[test]
    fn empty_desktop_just_paints_the_backdrop() {
        let desk = Desktop::new(rect(0, 0, 3, 2), Cell::from_char('░', Style::new()), vec![]);
        assert_eq!(desk.active(), None);
        assert!(!desk.focusable());
        let mut buf = Buffer::new(Size::new(3, 2));
        let mut canvas = Canvas::new(&mut buf);
        desk.draw(&mut canvas);
        assert_eq!(buf.to_text(), "░░░\n░░░");
    }

    #[test]
    fn a_window_casts_its_drop_shadow_on_the_backdrop() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let shadow = Theme::default().style(Role::Shadow);
        // An 8×4 window at (2, 1), clear of the surface edges so its whole shadow
        // lands on the backdrop: the right strip starts at x = 10 (ADR 0020).
        let desk = Desktop::new(
            rect(0, 0, 20, 10),
            Cell::from_char('░', Style::new()),
            vec![window(1, rect(2, 1, 8, 4), &log)],
        );
        let mut buf = Buffer::new(Size::new(20, 10));
        let mut canvas = Canvas::new(&mut buf);
        desk.draw(&mut canvas);

        // The shadow keeps the backdrop glyph but is repainted in the shadow style.
        let shadowed = buf.get(Point::new(10, 2)).unwrap();
        assert_eq!(shadowed.grapheme().to_string(), "░");
        assert_eq!(shadowed.style(), shadow);
        // Backdrop clear of the window and its shadow is left alone.
        assert_eq!(buf.get(Point::new(0, 9)).unwrap().style(), Style::new());
    }

    #[test]
    fn topmost_window_is_active() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let desk = Desktop::new(
            rect(0, 0, 40, 12),
            Cell::default(),
            vec![
                window(1, rect(0, 0, 10, 5), &log),
                window(2, rect(5, 2, 10, 5), &log),
            ],
        );
        assert_eq!(desk.active(), Some(1));
    }

    #[test]
    fn keys_reach_only_the_active_window() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut desk = Desktop::new(
            rect(0, 0, 40, 12),
            Cell::default(),
            vec![
                window(1, rect(0, 0, 10, 5), &log),
                window(2, rect(12, 0, 10, 5), &log),
            ],
        );
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        desk.handle_event(
            &Event::Key(KeyEvent::new(KeyCode::Char('a'), Modifiers::NONE)),
            &mut ctx,
        );
        let seen = log.borrow();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, 2, "only the active (top) window saw the key");
    }

    #[test]
    fn a_command_from_the_active_window_bubbles_to_the_context() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut desk = Desktop::new(
            rect(0, 0, 40, 12),
            Cell::default(),
            vec![window(1, rect(0, 0, 10, 5), &log)],
        );
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        // Enter lands on the active window's interior, which posts CM_OK.
        desk.handle_event(
            &Event::Key(KeyEvent::new(KeyCode::Enter, Modifiers::NONE)),
            &mut ctx,
        );
        assert_eq!(ctx.posted(), &[Event::Command(CM_OK)]);
    }

    #[test]
    fn a_click_goes_to_the_topmost_window_under_it() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut desk = Desktop::new(
            rect(0, 0, 40, 12),
            Cell::default(),
            // Two overlapping windows; window 2 (later) is on top.
            vec![
                window(1, rect(0, 0, 12, 6), &log),
                window(2, rect(3, 2, 12, 6), &log),
            ],
        );
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        // (5,4) is inside both; the top window 2 must claim it, in its local coords
        // and inset into its interior.
        desk.handle_event(
            &Event::Mouse(MouseEvent {
                kind: MouseKind::Down(MouseButton::Left),
                pos: Point::new(5, 4),
                modifiers: Modifiers::NONE,
            }),
            &mut ctx,
        );
        let seen = log.borrow();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, 2);
    }
}
