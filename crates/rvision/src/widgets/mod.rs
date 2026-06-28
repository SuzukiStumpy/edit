//! The Phase 4 chrome widget family: the concrete [`View`](crate::view::View)s
//! that make a screen look like TurboVision — a desktop backdrop, framed windows,
//! a status line, and a menu bar with pull-downs.
//!
//! These are reusable and editor-agnostic — the furniture around the focusable,
//! content-bearing widgets (editor, dialog controls) that arrive in later phases.
//! The application root that lays them out, orders their drawing, draws the menu
//! overlay, and routes accelerators is [`crate::app::Shell`] (ADR 0016).

mod background;
mod button;
mod check_box;
mod desktop;
mod dialog;
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
pub use frame::Frame;
pub use input_line::InputLine;
pub use label::Label;
pub use list_box::ListBox;
pub use menu::{Menu, MenuBar, MenuItem};
pub use message_box::MessageBox;
pub use radio_buttons::RadioButtons;
pub use scroll_bar::ScrollBar;
pub use status::{StatusItem, StatusLine};
pub use window::Window;
