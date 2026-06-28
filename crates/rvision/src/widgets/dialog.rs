//! A modal dialog box (TurboVision's `TDialog`).
//!
//! A bordered, titled box of [controls](super) that runs *modally* over the
//! current screen via [`Application::exec_view`](crate::app::Application::exec_view)
//! and yields the command that closed it (ADR 0017). Unlike a
//! [`Window`](super::Window) a dialog is not on the desktop and never joins the
//! application's view tree: it is created, run by `exec_view`, and dropped.
//!
//! The controls are owned by an inner [`Group`], so the focused control draws
//! focused and `Tab` cycles them (ADR 0017). The dialog adds the modal manners on
//! top: `Esc` cancels, `Enter` activates the default button, and a small set of
//! *ending* commands stop the modal loop.

use crate::canvas::Canvas;
use crate::cell::Cell;
use crate::color::Style;
use crate::command::{CM_CANCEL, CM_OK, Command};
use crate::event::{Event, EventResult, KeyCode, MouseEvent};
use crate::geometry::{Point, Rect, Size};
use crate::theme::{Role, Theme};
use crate::view::{Context, Group, View};

/// A modal box of controls with a title and a border.
pub struct Dialog {
    size: Size,
    title: String,
    style: Style,
    controls: Group,
    ending: Vec<Command>,
    default_cmd: Option<Command>,
}

impl Dialog {
    /// Creates a dialog of `size` titled `title`, laying out `controls` (each
    /// positioned in the dialog's **interior** coordinates — `(0, 0)` is the first
    /// cell inside the border). Colours come from [`Role::DialogBackground`].
    /// Ends on `CM_OK`/`CM_CANCEL` by default.
    pub fn new(size: Size, title: &str, theme: &Theme, controls: Vec<Box<dyn View>>) -> Self {
        let interior = interior_of(size);
        Self {
            size,
            title: title.to_string(),
            style: theme.style(Role::DialogBackground),
            controls: Group::new(interior, controls),
            ending: vec![CM_OK, CM_CANCEL],
            default_cmd: None,
        }
    }

    /// Sets the default button's command — what the dialog posts when `Enter` is
    /// pressed and the focused control did not consume it.
    pub fn with_default(mut self, command: Command) -> Self {
        self.default_cmd = Some(command);
        self
    }

    /// Registers `command` as also ending the modal loop (beyond the default
    /// `CM_OK`/`CM_CANCEL`), e.g. `CM_YES`/`CM_NO`.
    pub fn also_ends_on(mut self, command: Command) -> Self {
        if !self.ending.contains(&command) {
            self.ending.push(command);
        }
        self
    }

    /// Whether posting `command` should close the modal loop.
    pub fn ends_on(&self, command: Command) -> bool {
        self.ending.contains(&command)
    }

    /// The dialog's size — [`exec_view`](crate::app::Application::exec_view)
    /// centres a box this big.
    pub fn size(&self) -> Size {
        self.size
    }

    /// The interior rectangle (the box inset one cell on every side) in the
    /// dialog's local coordinates.
    fn interior_bounds(&self) -> Rect {
        interior_of(self.size)
    }
}

/// The interior rect for a dialog of `size`: inset one cell on every side.
fn interior_of(size: Size) -> Rect {
    Rect::from_origin_size(
        Point::new(1, 1),
        Size::new((size.width - 2).max(0), (size.height - 2).max(0)),
    )
}

impl View for Dialog {
    fn bounds(&self) -> Rect {
        Rect::from_origin_size(Point::new(0, 0), self.size)
    }

    fn draw(&self, canvas: &mut Canvas) {
        let area = canvas.bounds();
        canvas.fill(area, &Cell::blank(self.style));
        canvas.draw_box(area, self.style);
        // Centre " title " on the top border.
        if !self.title.is_empty() && area.width() > 4 {
            let label = format!(" {} ", self.title);
            let len = label.chars().count() as i16;
            let x = ((area.width() - len) / 2).max(1);
            canvas.put_str(Point::new(x, 0), &label, self.style);
        }
        let interior = self.interior_bounds();
        if !interior.is_empty() {
            let mut sub = canvas.child(interior);
            self.controls.draw(&mut sub);
        }
    }

    fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        match event {
            Event::Mouse(mouse) => {
                let interior = self.interior_bounds();
                if interior.contains(mouse.pos) {
                    let local = MouseEvent {
                        pos: mouse.pos.offset(-interior.origin().x, -interior.origin().y),
                        ..*mouse
                    };
                    self.controls.handle_event(&Event::Mouse(local), ctx)
                } else {
                    EventResult::Ignored
                }
            }
            Event::Key(key) => {
                // Esc always cancels, before any control sees it.
                if key.code == KeyCode::Esc {
                    ctx.post(CM_CANCEL);
                    return EventResult::Consumed;
                }
                // The focused control gets first crack (Tab, button, typing).
                if self.controls.handle_event(event, ctx).is_consumed() {
                    return EventResult::Consumed;
                }
                // Enter falls back to the default button.
                if key.code == KeyCode::Enter {
                    if let Some(command) = self.default_cmd {
                        ctx.post(command);
                        return EventResult::Consumed;
                    }
                }
                EventResult::Ignored
            }
            // Commands (re-dispatched by exec_view) and broadcasts go to the controls.
            _ => self.controls.handle_event(event, ctx),
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::command::{CM_USER, CM_YES, CommandSet};
    use crate::event::{KeyEvent, Modifiers};
    use crate::widgets::{Button, Label};

    const CM_APPLY: Command = Command(CM_USER + 1);

    fn theme() -> Theme {
        Theme::default()
    }

    fn rect(x: i16, y: i16, w: i16, h: i16) -> Rect {
        Rect::from_origin_size(Point::new(x, y), Size::new(w, h))
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, Modifiers::NONE))
    }

    /// A dialog with two buttons: OK (default, at interior 1,3) and Cancel.
    fn two_button_dialog() -> Dialog {
        let t = theme();
        Dialog::new(
            Size::new(24, 7),
            "Confirm",
            &t,
            vec![
                Box::new(Label::new(rect(1, 1, 20, 1), "Proceed?", &t)),
                Box::new(Button::new(rect(1, 3, 8, 1), "OK", CM_OK, &t).default(true)),
                Box::new(Button::new(rect(11, 3, 10, 1), "Cancel", CM_CANCEL, &t)),
            ],
        )
        .with_default(CM_OK)
    }

    #[test]
    fn ends_on_standard_and_added_commands() {
        let d = Dialog::new(Size::new(10, 5), "T", &theme(), vec![]).also_ends_on(CM_YES);
        assert!(d.ends_on(CM_OK));
        assert!(d.ends_on(CM_CANCEL));
        assert!(d.ends_on(CM_YES));
        assert!(!d.ends_on(CM_APPLY), "an unrelated command does not end it");
    }

    #[test]
    fn esc_posts_cancel() {
        let mut d = two_button_dialog();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        assert_eq!(
            d.handle_event(&key(KeyCode::Esc), &mut ctx),
            EventResult::Consumed
        );
        assert_eq!(ctx.posted(), &[Event::Command(CM_CANCEL)]);
    }

    #[test]
    fn enter_on_the_default_button_posts_ok() {
        // Focus starts on the first focusable control — the OK button — so Enter
        // is consumed by the focused button itself.
        let mut d = two_button_dialog();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        assert_eq!(
            d.handle_event(&key(KeyCode::Enter), &mut ctx),
            EventResult::Consumed
        );
        assert_eq!(ctx.posted(), &[Event::Command(CM_OK)]);
    }

    #[test]
    fn enter_falls_back_to_the_default_when_focus_ignores_it() {
        // A dialog whose only focusable control declines Enter: the dialog's
        // default button command fires instead.
        let t = theme();
        let mut d = Dialog::new(
            Size::new(20, 5),
            "T",
            &t,
            vec![Box::new(IgnoreEnter {
                bounds: rect(1, 1, 5, 1),
            })],
        )
        .with_default(CM_APPLY);
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        d.handle_event(&key(KeyCode::Enter), &mut ctx);
        assert_eq!(ctx.posted(), &[Event::Command(CM_APPLY)]);
    }

    #[test]
    fn tab_moves_focus_between_buttons() {
        let mut d = two_button_dialog();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        // Tab to the Cancel button, then Enter activates it (the focused control).
        d.handle_event(&key(KeyCode::Tab), &mut ctx);
        let mut ctx = Context::new(&cs);
        d.handle_event(&key(KeyCode::Enter), &mut ctx);
        assert_eq!(ctx.posted(), &[Event::Command(CM_CANCEL)]);
    }

    #[test]
    fn snapshot_dialog_box() {
        let d = two_button_dialog();
        let mut buf = Buffer::new(Size::new(24, 7));
        let mut canvas = Canvas::new(&mut buf);
        d.draw(&mut canvas);
        insta::assert_snapshot!(buf.to_text());
    }

    /// A focusable control that ignores every key (to exercise the default-button
    /// fallback path).
    struct IgnoreEnter {
        bounds: Rect,
    }
    impl View for IgnoreEnter {
        fn bounds(&self) -> Rect {
            self.bounds
        }
        fn draw(&self, _canvas: &mut Canvas) {}
        fn focusable(&self) -> bool {
            true
        }
    }
}
