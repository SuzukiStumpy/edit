//! Diagnostic: dump every event crossterm delivers, raw and unmapped, so we can
//! see exactly what a given terminal sends — is a paste one `Paste(..)`, a flood
//! of `Key(..)`, or (for a middle-click) a `Mouse(..)`? Isolates the
//! terminal + crossterm layer from the rest of `rvision` (ADR 0022 follow-up).
//!
//! Run it, paste however you normally would, then press Esc to quit:
//!
//! ```sh
//! cargo run -p rvision --example event_probe
//! ```

use std::io::{self, Write};

use crossterm::ExecutableCommand;
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode,
};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut out = io::stdout();
    out.execute(EnableMouseCapture)?;
    out.execute(EnableBracketedPaste)?;
    // Raw mode: end every line with \r\n ourselves.
    print!("event_probe — paste now; Esc to quit.\r\n");
    out.flush()?;

    let result = loop {
        let ev = match event::read() {
            Ok(ev) => ev,
            Err(e) => break Err(e),
        };
        print!("{ev:?}\r\n");
        if out.flush().is_err() {
            break Ok(());
        }
        if matches!(ev, Event::Key(k) if k.code == KeyCode::Esc) {
            break Ok(());
        }
    };

    let _ = out.execute(DisableBracketedPaste);
    let _ = out.execute(DisableMouseCapture);
    let _ = disable_raw_mode();
    result
}
