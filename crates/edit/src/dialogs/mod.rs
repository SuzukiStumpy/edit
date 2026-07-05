//! The editor's own modal dialogs (Go to Line, Find, Replace, Settings).
//!
//! These are editor concepts, so they live in the `edit` crate rather than in the
//! editor-agnostic `rvision` framework, but they are composed from generic
//! `rvision` controls. `rvision`'s `Modal` trait is gone (its ADR 0016 unified
//! `Window`/`Dialog`, so [`exec_view`](rvision::app::Application::exec_view)
//! now runs a concrete `Window` rather than any `View` a caller hands it) —
//! each dialog is instead run through `modal_window`, which boxes it as a
//! `Window`'s interior and hands back a shared `Rc<RefCell<T>>` handle the
//! driver reads the typed value back through once `exec_view` returns `CM_OK`.
//! This is the same shared-cell idiom `rvision::widgets::FileDialog` itself now
//! uses to return a chosen path past its own `Box<dyn View>` interior. See
//! `docs/specs/editor-dialogs.md`.

use std::cell::RefCell;
use std::rc::Rc;

use rvision::canvas::Canvas;
use rvision::command::{CM_CANCEL, CM_OK};
use rvision::event::{Event, EventResult};
use rvision::geometry::{Point, Rect, Size};
use rvision::theme::Theme;
use rvision::view::{Context, View};
use rvision::widgets::Window;

mod find;
mod go_to_line;
mod replace;
mod settings;

pub use find::FindDialog;
pub use go_to_line::GoToLine;
pub use replace::ReplaceDialog;
pub use settings::{CM_DEFAULTS, SettingsDialog};

/// Forwards every [`View`] method to a shared, ref-counted `T` — the narrow
/// seam that lets a caller keep reading a dialog's own typed accessors
/// (`GoToLine::line`, `FindDialog::query`, ...) after the dialog itself has
/// been boxed away as a [`Window`]'s `Box<dyn View>` interior.
struct Shared<T>(Rc<RefCell<T>>);

impl<T: View> View for Shared<T> {
    fn bounds(&self) -> Rect {
        self.0.borrow().bounds()
    }

    fn draw(&self, canvas: &mut Canvas) {
        self.0.borrow().draw(canvas)
    }

    fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        self.0.borrow_mut().handle_event(event, ctx)
    }

    fn focusable(&self) -> bool {
        self.0.borrow().focusable()
    }
}

/// Wraps `dialog` as a centred, `Esc`-cancelling, fixed-size [`Window`] of
/// `outer_size` titled `title`, ending on `CM_OK` (also `dialog`'s own
/// `Enter`-fallback default) or `CM_CANCEL` — the shape every editor dialog
/// (Go to Line, Find, Replace, Settings) shares. `outer_size` includes the
/// one-cell border the `Window`'s `Frame` draws; `dialog`'s own
/// [`View::bounds`] should report just its content area (ADR 0016 moved
/// border/title drawing to `Window`, out of the dialog itself).
///
/// Returns the `Window` to run via
/// [`Application::exec_view`](rvision::app::Application::exec_view), plus a
/// handle: `handle.borrow()` reads `dialog`'s live state, valid for as long
/// as the window is (and still after it closes, since nothing else holds it).
pub(crate) fn modal_window<T: View + 'static>(
    title: &str,
    outer_size: Size,
    theme: &Theme,
    dialog: T,
) -> (Window, Rc<RefCell<T>>) {
    let handle = Rc::new(RefCell::new(dialog));
    let interior: Box<dyn View> = Box::new(Shared(handle.clone()));
    let bounds = Rect::from_origin_size(Point::new(0, 0), outer_size);
    let window = Window::dialog(bounds, title, theme, interior)
        .centered()
        .resizable(false)
        .zoomable(false)
        .closable(false)
        .also_ends_on(CM_OK)
        .also_ends_on(CM_CANCEL)
        .with_default(CM_OK)
        .esc_cancels(true);
    (window, handle)
}
