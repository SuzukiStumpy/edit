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
use crate::wrap;
use unicode_width::UnicodeWidthStr;

use super::{Button, Dialog, Label};

/// Maximum interior message width in display columns before a line is wrapped.
/// Callers pass prose; pre-split short lines (ADR 0022) stay untouched because
/// hard breaks survive and each is already under this width.
const MAX_WIDTH: i16 = 50;

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

    /// A three-way question — `Yes` (default) / `No` / `Cancel`
    /// (`CM_YES`/`CM_NO`/`CM_CANCEL`) — for "save changes before…?" prompts.
    pub fn yes_no_cancel(title: &str, message: &str, theme: &Theme) -> Dialog {
        build(
            title,
            message,
            &[("Yes", CM_YES), ("No", CM_NO), ("Cancel", CM_CANCEL)],
            theme,
        )
    }
}

/// One interior cell of horizontal padding on each side; the box is two rows of
/// border plus message / gap / buttons.
fn build(title: &str, message: &str, buttons: &[(&str, Command)], theme: &Theme) -> Dialog {
    const PAD: i16 = 2; // interior horizontal padding each side
    const GAP: i16 = 2; // columns between buttons
    const MSG_TOP: i16 = 1; // first message row (row 0 is top padding)

    // Wrap to a sane width, then each line becomes its own centred Label so the
    // box grows to fit rather than spilling off one row. Wrapping preserves hard
    // '\n' breaks, so callers that pre-split short lines (ADR 0022) are untouched.
    let wrapped = wrap::wrap(message, MAX_WIDTH as u16);
    let lines: Vec<&str> = wrapped.iter().map(String::as_str).collect();
    let line_w = |s: &str| s.width() as i16;
    let msg_w = lines.iter().map(|l| line_w(l)).max().unwrap_or(0);
    let n = lines.len() as i16;

    let btn_w = |label: &str| label.chars().count() as i16 + 4;
    let buttons_w: i16 = buttons.iter().map(|(l, _)| btn_w(l)).sum::<i16>()
        + GAP * (buttons.len() as i16 - 1).max(0);

    let content_w = msg_w.max(buttons_w);
    let interior_w = content_w + 2 * PAD;
    let btn_row = MSG_TOP + n + 1; // a blank gap row between message and buttons
    let interior_h = btn_row + 2; // buttons row + a bottom padding row
    let size = Size::new(interior_w + 2, interior_h + 2);

    let mut controls: Vec<Box<dyn View>> = Vec::with_capacity(lines.len() + buttons.len());
    for (i, line) in lines.iter().enumerate() {
        let w = line_w(line);
        if w == 0 {
            continue; // a blank line just spaces things out; the dialog fills it
        }
        let x = (interior_w - w) / 2;
        controls.push(Box::new(Label::new(
            Rect::from_origin_size(Point::new(x, MSG_TOP + i as i16), Size::new(w, 1)),
            line,
            theme,
        )));
    }

    let mut x = (interior_w - buttons_w) / 2;
    for (i, (label, command)) in buttons.iter().enumerate() {
        let w = btn_w(label);
        controls.push(Box::new(
            Button::new(
                Rect::from_origin_size(Point::new(x, btn_row), Size::new(w, 1)),
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
    fn yes_no_cancel_ends_on_all_three_answers() {
        let d = MessageBox::yes_no_cancel("Exit", "Save changes?", &Theme::default());
        assert!(d.ends_on(CM_YES));
        assert!(d.ends_on(CM_NO));
        assert!(d.ends_on(CM_CANCEL));
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

    #[test]
    fn each_message_line_adds_one_row_to_the_box() {
        let one = MessageBox::ok("T", "One line.", &Theme::default());
        // A blank middle line counts too — it is the spacer that previously bled.
        let three = MessageBox::ok("T", "Line one.\n\nLine three.", &Theme::default());
        assert_eq!(three.size().height - one.size().height, 2);
    }

    #[test]
    fn a_long_unbroken_message_wraps_to_keep_the_box_narrow() {
        let long = "This is a single long line of prose that the message box \
                    should wrap across several rows instead of letting it run \
                    off the edge of the screen.";
        let one = MessageBox::ok("Info", "short", &Theme::default());
        let d = MessageBox::ok("Info", long, &Theme::default());
        // It wrapped: the box stays near the wrap width, not the raw length…
        assert!(
            d.size().width <= MAX_WIDTH + 6,
            "box width {} stayed bounded",
            d.size().width
        );
        assert!(
            (d.size().width as usize) < long.chars().count(),
            "and is far narrower than the unwrapped message"
        );
        // …and grew taller, one extra row per wrapped line.
        assert!(d.size().height > one.size().height);
    }

    #[test]
    fn pre_split_lines_under_the_width_are_left_alone() {
        // A caller that already split on '\n' (ADR 0022) keeps its layout: each
        // short hard line stays its own row, none are merged.
        let d = MessageBox::ok("T", "line one\nline two\nline three", &Theme::default());
        let one = MessageBox::ok("T", "line one", &Theme::default());
        assert_eq!(d.size().height - one.size().height, 2);
    }

    #[test]
    fn snapshot_multiline_message_box() {
        // The box grows to hold every line; the blank row is filled by the dialog,
        // and no '\n' ever lands in a cell (each line is its own Label).
        let d = MessageBox::ok(
            "Paste",
            "Clipboard is empty.\n\nUse Ctrl+Shift+V.",
            &Theme::default(),
        );
        let size = d.size();
        let mut buf = Buffer::new(size);
        let mut canvas = Canvas::new(&mut buf);
        d.draw(&mut canvas);
        insta::assert_snapshot!(buf.to_text());
    }
}
