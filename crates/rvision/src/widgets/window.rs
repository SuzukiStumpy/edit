//! A framed window: a [`Frame`] around an interior [`View`].
//!
//! The interior is whatever the window contains — a placeholder now, an editor
//! later (Phase 6). The window draws its frame over its whole rectangle, then the
//! interior through a sub-canvas inset by one cell on every side, and routes
//! events the interior cares about into it. Dragging, resizing, and the close/zoom
//! buttons become live in Phase 9 (ADR 0007); here the frame's glyphs are drawn
//! but inert.

use crate::canvas::Canvas;
use crate::cell::Cell;
use crate::color::Style;
use crate::event::{Event, EventResult, MouseEvent};
use crate::geometry::{Point, Rect, Size};
use crate::theme::{Role, Theme};
use crate::view::{Context, View};

use super::Frame;

/// A window: a titled frame plus an interior view.
pub struct Window {
    bounds: Rect,
    frame: Frame,
    active: bool,
    interior_fill: Cell,
    shadow_style: Style,
    casts_shadow: bool,
    interior: Box<dyn View>,
}

impl Window {
    /// Creates a window at `bounds` titled `title`, taking its frame/title colours
    /// from `theme`, wrapping `interior`. Inactive until the desktop activates it.
    pub fn new(bounds: Rect, title: &str, theme: &Theme, interior: Box<dyn View>) -> Self {
        let frame = Frame::new(
            title,
            theme.style(Role::WindowFrame),
            theme.style(Role::WindowTitle),
        );
        Self {
            bounds,
            frame,
            active: false,
            interior_fill: Cell::blank(theme.style(Role::WindowFrame)),
            shadow_style: theme.style(Role::Shadow),
            casts_shadow: true,
            interior,
        }
    }

    /// Sets whether this window casts a drop shadow on what lies behind it
    /// (default `true`). Turn it off for a window meant to sit flush — e.g. one
    /// maximised to fill the desktop, whose shadow would only fall off-screen
    /// (ADR 0020).
    pub fn set_casts_shadow(&mut self, casts: bool) {
        self.casts_shadow = casts;
    }

    /// The interior rectangle in the window's **local** coordinates: the whole
    /// window inset by one cell on every side (the border). Collapses to empty for
    /// a window too small to have an interior.
    pub fn interior_bounds(&self) -> Rect {
        let Size { width, height } = self.bounds.size();
        Rect::from_origin_size(
            Point::new(1, 1),
            Size::new((width - 2).max(0), (height - 2).max(0)),
        )
    }

    /// Whether the window is currently the active (focused) one.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Marks the window active or not, switching its frame between the doubled
    /// (active) and single (inactive) border.
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
        self.frame.set_active(active);
    }
}

impl View for Window {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn draw(&self, canvas: &mut Canvas) {
        self.frame.draw(canvas);
        let interior = self.interior_bounds();
        if !interior.is_empty() {
            let mut sub = canvas.child(interior);
            // Paint the window's own background so a non-filling interior (or none)
            // shows solid window colour, not the desktop bleeding through.
            let area = sub.bounds();
            sub.fill(area, &self.interior_fill);
            self.interior.draw(&mut sub);
        }
    }

    fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        match event {
            // Positional events inside the interior are translated into it; clicks
            // on the border itself are ignored (the frame's buttons wake in Phase 9).
            Event::Mouse(mouse) => {
                let interior = self.interior_bounds();
                if interior.contains(mouse.pos) {
                    let local = MouseEvent {
                        pos: mouse.pos.offset(-interior.origin().x, -interior.origin().y),
                        ..*mouse
                    };
                    self.interior.handle_event(&Event::Mouse(local), ctx)
                } else {
                    EventResult::Ignored
                }
            }
            // Everything else (keys, commands, broadcasts) goes straight to the
            // interior, whose ignored results bubble back up (ADR 0003).
            _ => self.interior.handle_event(event, ctx),
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn drop_shadow(&self) -> Option<Style> {
        self.casts_shadow.then_some(self.shadow_style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::canvas::Canvas;
    use crate::color::Style;
    use crate::command::{CM_OK, CommandSet};
    use crate::event::{KeyCode, KeyEvent, Modifiers};
    use crate::view::StaticText;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn rect(x: i16, y: i16, w: i16, h: i16) -> Rect {
        Rect::from_origin_size(Point::new(x, y), Size::new(w, h))
    }

    /// An interior that records keys and posts `CM_OK` on Enter.
    struct Interior {
        seen: Rc<RefCell<Vec<Event>>>,
    }

    impl View for Interior {
        fn bounds(&self) -> Rect {
            rect(0, 0, 100, 100)
        }
        fn draw(&self, canvas: &mut Canvas) {
            canvas.put_str(Point::new(0, 0), "in", Style::new());
        }
        fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
            self.seen.borrow_mut().push(*event);
            if matches!(event, Event::Key(k) if k.code == KeyCode::Enter) {
                ctx.post(CM_OK);
                return EventResult::Consumed;
            }
            EventResult::Ignored
        }
    }

    #[test]
    fn interior_bounds_inset_by_one_on_each_side() {
        let w = Window::new(
            rect(5, 2, 20, 8),
            "T",
            &Theme::default(),
            Box::new(StaticText::new(rect(0, 0, 1, 1), "", Style::new())),
        );
        assert_eq!(w.interior_bounds(), rect(1, 1, 18, 6));
    }

    #[test]
    fn casts_a_shadow_by_default_and_can_be_turned_off() {
        let theme = Theme::default();
        let mut w = Window::new(
            rect(5, 2, 20, 8),
            "T",
            &theme,
            Box::new(StaticText::new(rect(0, 0, 1, 1), "", Style::new())),
        );
        // Default: the window reports the theme's shadow style for its owner to
        // paint (ADR 0020).
        assert_eq!(w.drop_shadow(), Some(theme.style(Role::Shadow)));
        // Turning it off makes it sit flush — no shadow reported.
        w.set_casts_shadow(false);
        assert_eq!(w.drop_shadow(), None);
    }

    #[test]
    fn tiny_window_has_an_empty_interior() {
        let w = Window::new(
            rect(0, 0, 1, 1),
            "T",
            &Theme::default(),
            Box::new(StaticText::new(rect(0, 0, 1, 1), "", Style::new())),
        );
        assert!(w.interior_bounds().is_empty());
    }

    #[test]
    fn snapshot_window_draws_frame_then_interior() {
        let w = Window::new(
            rect(0, 0, 28, 5),
            "Untitled",
            &Theme::default(),
            Box::new(StaticText::new(rect(0, 0, 26, 3), "hello", Style::new())),
        );
        let mut buf = Buffer::new(Size::new(28, 5));
        let mut canvas = Canvas::new(&mut buf);
        w.draw(&mut canvas);
        insta::assert_snapshot!(buf.to_text());
    }

    #[test]
    fn keys_route_into_the_interior_and_commands_bubble() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut w = Window::new(
            rect(0, 0, 16, 5),
            "T",
            &Theme::default(),
            Box::new(Interior {
                seen: Rc::clone(&seen),
            }),
        );
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);

        let enter = Event::Key(KeyEvent::new(KeyCode::Enter, Modifiers::NONE));
        assert_eq!(w.handle_event(&enter, &mut ctx), EventResult::Consumed);
        assert_eq!(*seen.borrow(), vec![enter]);
        assert_eq!(ctx.posted(), &[Event::Command(CM_OK)]);
    }

    #[test]
    fn set_active_toggles_the_frame_and_flag() {
        let mut w = Window::new(
            rect(0, 0, 10, 4),
            "T",
            &Theme::default(),
            Box::new(StaticText::new(rect(0, 0, 1, 1), "", Style::new())),
        );
        assert!(!w.is_active());
        w.set_active(true);
        assert!(w.is_active());
    }
}
