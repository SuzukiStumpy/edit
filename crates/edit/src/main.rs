//! The `edit` binary — entry point.
//!
//! Builds the editor application over a real terminal and runs its driver loop
//! (ADR 0018). An optional path argument opens that file (or starts a new
//! document to be created there). The panic-safe RAII backend always restores the
//! terminal, even on a crash (ADR 0001).

use std::io;
use std::time::Duration;

use edit::app::{EditorApp, run};
use rvision::app::Application;
use rvision::backend::Backend;
use rvision::crossterm_backend::CrosstermBackend;
use rvision::theme::Theme;

fn main() -> io::Result<()> {
    let backend = CrosstermBackend::new()?;
    let size = backend.size();
    let theme = Theme::default();

    let mut editor = EditorApp::new(size, &theme);
    if let Some(path) = std::env::args_os().nth(1) {
        // A read error here is non-fatal: start empty so the editor still opens.
        let _ = editor.open_or_new(path);
    }

    let app = Application::new(backend).with_timeout(Duration::from_millis(250));
    run(app, editor, &theme)
    // `app` (and the backend) drops in `run`, restoring the terminal.
}
