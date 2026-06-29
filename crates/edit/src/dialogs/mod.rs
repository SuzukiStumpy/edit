//! The editor's own modal dialogs (Go to Line, Find, Replace).
//!
//! These are editor concepts, so they live in the `edit` crate rather than in the
//! editor-agnostic `rvision` framework, but they are composed from generic
//! `rvision` controls. Each owns its controls **concretely** (like
//! [`FileDialog`](rvision::widgets::FileDialog)) so the driver can read the typed
//! value back after [`exec_view`](rvision::app::Application::exec_view) returns
//! `CM_OK` — no downcast, no view IDs (ADR 0017/0018). See
//! `docs/specs/editor-dialogs.md`.

mod find;
mod go_to_line;
mod replace;

pub use find::FindDialog;
pub use go_to_line::GoToLine;
pub use replace::ReplaceDialog;
