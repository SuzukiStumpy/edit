//! Phase 2 manual check: take over the terminal, paint a TurboVision-ish screen,
//! and run the real event loop until Ctrl-Q (or Esc).
//!
//! Run with `cargo run -p rvision --example hello`. This is the roadmap's first
//! on-a-real-terminal verification: it should draw cleanly, advance the idle
//! spinner ~4×/second, repaint on resize, and *always* restore the terminal on
//! exit — even if it panics (try the commented `panic!` in `handle_event`).

use std::io;
use std::time::Duration;

use rvision::app::{Application, Program};
use rvision::buffer::Buffer;
use rvision::cell::Cell;
use rvision::crossterm_backend::CrosstermBackend;
use rvision::event::{Event, EventResult, KeyCode, Modifiers};
use rvision::geometry::{Point, Rect, Size};
use rvision::theme::{Role, Theme};

/// The spinner glyphs cycled on each idle tick, to prove the timeout path works.
const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

struct Demo {
    theme: Theme,
    finished: bool,
    ticks: u64,
}

impl Demo {
    fn new() -> Self {
        Self {
            theme: Theme::default(),
            finished: false,
            ticks: 0,
        }
    }
}

impl Program for Demo {
    fn draw(&mut self, frame: &mut Buffer) {
        let size = frame.size();

        // Desktop backdrop.
        frame.fill(
            frame.bounds(),
            &Cell::blank(self.theme.style(Role::DesktopBackground)),
        );

        // A centred window.
        let w = 46.min(size.width);
        let h = 9.min(size.height);
        if w < 4 || h < 3 {
            return; // too small to bother
        }
        let origin = Point::new((size.width - w) / 2, (size.height - h) / 2);
        let window = Rect::from_origin_size(origin, Size::new(w, h));
        let frame_style = self.theme.style(Role::WindowFrame);
        frame.fill(window, &Cell::blank(frame_style));
        frame.draw_box(window, frame_style);

        // Title on the top border, then body text inside.
        let title_style = self.theme.style(Role::WindowTitle);
        frame.put_str(origin.offset(2, 0), " rvision · hello ", title_style);

        let text = origin.offset(2, 2);
        frame.put_str(text, "The Phase 2 event loop is live.", frame_style);
        frame.put_str(
            text.offset(0, 2),
            &format!(
                "Terminal: {}×{}   idle {}",
                size.width,
                size.height,
                SPINNER[(self.ticks % 4) as usize]
            ),
            frame_style,
        );

        let hint_style = self.theme.style(Role::StatusKey);
        frame.put_str(text.offset(0, 4), "Press Ctrl-Q or Esc to quit", hint_style);
    }

    fn handle_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::Idle => {
                self.ticks += 1;
                EventResult::Consumed
            }
            Event::Key(key) => {
                // Uncomment to prove panic-safe restore:
                // if matches!(key.code, KeyCode::Char('p')) { panic!("boom"); }
                let quit = matches!(key.code, KeyCode::Esc)
                    || (matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q'))
                        && key.modifiers.contains(Modifiers::CONTROL));
                if quit {
                    self.finished = true;
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    fn is_finished(&self) -> bool {
        self.finished
    }
}

fn main() -> io::Result<()> {
    let backend = CrosstermBackend::new()?;
    let mut app = Application::new(backend).with_timeout(Duration::from_millis(250));
    let mut demo = Demo::new();
    app.run(&mut demo)
    // `app` (and thus the backend) drops here, restoring the terminal.
}
