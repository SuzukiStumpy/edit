//! Canned information / confirmation dialogs (TurboVision's `messageBox`).
//!
//! Each constructor builds a [`Dialog`](super::Dialog) with the message on one
//! row and a centred row of buttons below; the first button is the default and
//! every button's command ends the modal loop. Run it with
//! [`Application::exec_view`](crate::app::Application::exec_view); the returned
//! command says which button was pressed.

use crate::command::{CM_CANCEL, CM_NO, CM_OK, CM_YES, Command};
use crate::geometry::{Point, Rect, Size};
use crate::theme::Theme;
use crate::view::View;

use super::{Button, Dialog, Label};

/// Builders for the standard message boxes.
pub struct MessageBox;

impl MessageBox {
    /// An information box with a single `OK` button (returns `CM_OK`).
    pub fn ok(title: &str, message: &str, theme: &Theme) -> Dialog {
        build(title, message, &[("OK", CM_OK)], theme)
    }

    /// A confirmation box with `OK` (default) and `Cancel` (`CM_OK`/`CM_CANCEL`).
    pub fn ok_cancel(title: &str, message: &str, theme: &Theme) -> Dialog {
        build(
            title,
            message,
            &[("OK", CM_OK), ("Cancel", CM_CANCEL)],
            theme,
        )
    }

    /// A yes/no question with `Yes` (default) and `No` (`CM_YES`/`CM_NO`).
    pub fn yes_no(title: &str, message: &str, theme: &Theme) -> Dialog {
        build(title, message, &[("Yes", CM_YES), ("No", CM_NO)], theme)
    }
}

/// One interior cell of horizontal padding on each side; the box is two rows of
/// border plus message / gap / buttons.
fn build(title: &str, message: &str, buttons: &[(&str, Command)], theme: &Theme) -> Dialog {
    const PAD: i16 = 2; // interior horizontal padding each side
    const GAP: i16 = 2; // columns between buttons
    const MSG_ROW: i16 = 1;
    const BTN_ROW: i16 = 3;
    const INTERIOR_H: i16 = 5;

    let msg_w = message.chars().count() as i16;
    let btn_w = |label: &str| label.chars().count() as i16 + 4;
    let buttons_w: i16 = buttons.iter().map(|(l, _)| btn_w(l)).sum::<i16>()
        + GAP * (buttons.len() as i16 - 1).max(0);

    let content_w = msg_w.max(buttons_w);
    let interior_w = content_w + 2 * PAD;
    let size = Size::new(interior_w + 2, INTERIOR_H + 2);

    let mut controls: Vec<Box<dyn View>> = Vec::with_capacity(buttons.len() + 1);
    let msg_x = (interior_w - msg_w) / 2;
    controls.push(Box::new(Label::new(
        Rect::from_origin_size(Point::new(msg_x, MSG_ROW), Size::new(msg_w.max(0), 1)),
        message,
        theme,
    )));

    let mut x = (interior_w - buttons_w) / 2;
    for (i, (label, command)) in buttons.iter().enumerate() {
        let w = btn_w(label);
        controls.push(Box::new(
            Button::new(
                Rect::from_origin_size(Point::new(x, BTN_ROW), Size::new(w, 1)),
                label,
                *command,
                theme,
            )
            .default(i == 0),
        ));
        x += w + GAP;
    }

    let default_cmd = buttons[0].1;
    let mut dialog = Dialog::new(size, title, theme, controls).with_default(default_cmd);
    for (_, command) in buttons {
        dialog = dialog.also_ends_on(*command);
    }
    dialog
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::canvas::Canvas;
    use crate::command::CommandSet;
    use crate::event::{Event, EventResult, KeyCode, KeyEvent, Modifiers};
    use crate::view::Context;

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, Modifiers::NONE))
    }

    #[test]
    fn ok_cancel_enter_activates_the_default_ok() {
        let mut d = MessageBox::ok_cancel("Confirm", "Proceed?", &Theme::default());
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        // Focus starts on the default OK button; Enter posts CM_OK.
        assert_eq!(
            d.handle_event(&key(KeyCode::Enter), &mut ctx),
            EventResult::Consumed
        );
        assert_eq!(ctx.posted(), &[Event::Command(CM_OK)]);
    }

    #[test]
    fn yes_no_ends_on_both_answers() {
        let d = MessageBox::yes_no("Delete", "Delete file?", &Theme::default());
        assert!(d.ends_on(CM_YES));
        assert!(d.ends_on(CM_NO));
    }

    #[test]
    fn snapshot_message_box() {
        let d = MessageBox::yes_no("Confirm", "Save changes?", &Theme::default());
        let size = d.size();
        let mut buf = Buffer::new(size);
        let mut canvas = Canvas::new(&mut buf);
        d.draw(&mut canvas);
        insta::assert_snapshot!(buf.to_text());
    }
}
