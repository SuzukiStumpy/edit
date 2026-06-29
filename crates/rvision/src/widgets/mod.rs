//! The widget family: the concrete [`View`](crate::view::View)s that make a
//! screen look like TurboVision.
//!
//! - **Chrome (Phase 4):** a desktop backdrop, framed windows, a status line, and
//!   a menu bar with pull-downs — the furniture, laid out and routed by
//!   [`crate::app::Shell`] (ADR 0016).
//! - **Dialogs & controls (Phase 5):** [`Dialog`] (and [`MessageBox`],
//!   [`FileDialog`]) run modally via
//!   [`Application::exec_view`](crate::app::Application::exec_view), holding the
//!   focusable controls [`Button`], [`Label`], [`InputLine`], [`CheckBox`],
//!   [`RadioButtons`], [`ListBox`], and [`ScrollBar`]. Focus-aware drawing is the
//!   `set_focused` push (ADR 0017).
//!
//! All are reusable and editor-agnostic; the editor view itself arrives in Phase 6.

mod background;
mod button;
mod check_box;
mod desktop;
mod dialog;
mod file_dialog;
mod frame;
mod input_line;
mod label;
mod list_box;
mod menu;
mod message_box;
mod radio_buttons;
mod scroll_bar;
mod status;
mod window;

pub use background::Background;
pub use button::Button;
pub use check_box::CheckBox;
pub use desktop::Desktop;
pub use dialog::Dialog;
pub use file_dialog::FileDialog;
pub use frame::Frame;
pub use input_line::InputLine;
pub use label::Label;
pub use list_box::ListBox;
pub use menu::{Menu, MenuBar, MenuItem};
pub use message_box::MessageBox;
pub use radio_buttons::RadioButtons;
pub use scroll_bar::{Orientation, ScrollBar};
pub use status::{StatusItem, StatusLine};
pub use window::Window;
