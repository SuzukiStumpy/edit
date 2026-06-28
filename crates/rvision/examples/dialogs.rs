//! Phase 5 manual check: modal dialogs and controls driven by the keyboard,
//! over a plain blue backdrop, through the real event loop.
//!
//! Run with `cargo run -p rvision --example dialogs`. It shows, in sequence:
//!
//! 1. a welcome message box (`Enter`/`Esc`);
//! 2. a *Settings* dialog exercising every control — an input line, a check box,
//!    a radio group, and OK/Cancel buttons. `Tab`/`Shift-Tab` move focus, the
//!    arrows drive the radio group and (when focused) the input caret, `Space`
//!    toggles the check box, `Enter` is the default *OK*, `Esc` cancels;
//! 3. a file *Open* dialog — type a name or pick from the list; `Enter` on a
//!    folder navigates into it, `Enter` on a file (or *Open*) accepts;
//! 4. a closing box reporting what you picked.
//!
//! The chosen file and the settings result are printed after the terminal is
//! restored (the RAII backend, ADR 0001).

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use rvision::app::{Application, Program};
use rvision::buffer::Buffer;
use rvision::cell::Cell;
use rvision::color::Style;
use rvision::command::{CM_OK, Command};
use rvision::crossterm_backend::CrosstermBackend;
use rvision::event::{Event, EventResult};
use rvision::geometry::{Point, Rect, Size};
use rvision::theme::{Role, Theme};
use rvision::view::View;
use rvision::widgets::{
    Button, CheckBox, Dialog, FileDialog, InputLine, Label, MessageBox, RadioButtons,
};

fn rect(x: i16, y: i16, w: i16, h: i16) -> Rect {
    Rect::from_origin_size(Point::new(x, y), Size::new(w, h))
}

/// The screen behind the dialogs: a blue desktop with a title strip. It only
/// ever draws — `exec_view` never feeds it events.
struct Backdrop {
    style: Style,
}

impl Program for Backdrop {
    fn draw(&mut self, frame: &mut Buffer) {
        frame.fill(frame.bounds(), &Cell::from_char('░', self.style));
        frame.put_str(
            Point::new(2, 0),
            " rvision — Phase 5 dialogs demo (Esc cancels) ",
            self.style,
        );
    }

    fn handle_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }

    fn is_finished(&self) -> bool {
        false
    }
}

/// A custom dialog wiring up an input line, a check box, a radio group, and the
/// OK/Cancel buttons — interior coordinates have `(0, 0)` just inside the border.
fn settings_dialog(theme: &Theme) -> Dialog {
    let cancel = Command(3); // CM_CANCEL
    let controls: Vec<Box<dyn View>> = vec![
        Box::new(Label::new(rect(1, 1, 8, 1), "Name:", theme)),
        Box::new(InputLine::new(rect(9, 1, 22, 1), theme).with_text("untitled.txt")),
        Box::new(CheckBox::new(rect(1, 3, 28, 1), "Word wrap", theme).with_checked(true)),
        Box::new(Label::new(rect(1, 5, 14, 1), "Line endings:", theme)),
        Box::new(RadioButtons::new(
            rect(1, 6, 16, 3),
            &["Unix (LF)", "DOS (CRLF)", "Mac (CR)"],
            theme,
        )),
        Box::new(Button::new(rect(8, 10, 10, 1), "OK", CM_OK, theme).default(true)),
        Box::new(Button::new(rect(20, 10, 10, 1), "Cancel", cancel, theme)),
    ];
    Dialog::new(Size::new(34, 13), "Settings", theme, controls).with_default(CM_OK)
}

fn main() -> io::Result<()> {
    let backend = CrosstermBackend::new()?;
    let theme = Theme::default();
    let mut app = Application::new(backend).with_timeout(Duration::from_millis(250));
    let mut backdrop = Backdrop {
        style: theme.style(Role::DesktopBackground),
    };

    let mut welcome = MessageBox::ok("Welcome", "A tour of rvision dialogs.", &theme);
    app.exec_view(&mut backdrop, &mut welcome)?;

    let mut settings = settings_dialog(&theme);
    let settings_result = app.exec_view(&mut backdrop, &mut settings)?;

    let start = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut picker = FileDialog::open("Open File", start, &theme);
    let picked = if app.exec_view(&mut backdrop, &mut picker)? == CM_OK {
        Some(picker.path())
    } else {
        None
    };

    let summary = match &picked {
        Some(path) => format!("Opened: {}", path.display()),
        None => "No file opened.".to_string(),
    };
    let mut goodbye = MessageBox::ok("Done", &summary, &theme);
    app.exec_view(&mut backdrop, &mut goodbye)?;

    drop(app); // restores the terminal before we print to stdout
    println!("Settings closed with command {settings_result:?}");
    if let Some(path) = picked {
        println!("Picked file: {}", path.display());
    }
    Ok(())
}
