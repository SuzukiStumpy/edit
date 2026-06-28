//! The real-terminal backend: [`CrosstermBackend`] implements both halves of the
//! seam ([`Backend`] + [`EventSource`]) over a live terminal.
//!
//! This is the one module that names crossterm (ADR 0001): raw mode, the
//! alternate screen, output flushing, and input live here so the rest of the
//! framework depends only on our own traits and [`Event`] type. The only piece
//! that is unit-testable without a TTY — the pure crossterm → [`Event`] mapping —
//! is factored out as `map_event` and tested below; the terminal I/O itself is
//! verified by `examples/hello.rs` (the roadmap's Phase 2 manual check).

use crate::backend::{Backend, EventSource};
use crate::buffer::Buffer;
use crate::color::{Attributes, Color, Color16};
use crate::event::{Event, KeyCode, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseKind};
use crate::geometry::{Point, Size};
use std::io::{self, Write};
use std::sync::Once;
use std::time::Duration;

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event as ct;
use crossterm::style::{
    Attribute, Color as CtColor, Print, SetAttribute, SetBackgroundColor, SetForegroundColor,
};
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{ExecutableCommand, QueueableCommand};

/// A [`Backend`]/[`EventSource`] over a live terminal via crossterm.
///
/// Construction enters raw mode and the alternate screen and hides the cursor;
/// [`Drop`] reverses all three. Restoration is also wired into a panic hook, so a
/// crash leaves the terminal usable and its backtrace readable (ADR 0001).
pub struct CrosstermBackend {
    /// The last frame flushed to the screen — the diff baseline (ADR 0002).
    front: Buffer,
    /// The terminal's current size, refreshed on resize.
    size: Size,
}

impl CrosstermBackend {
    /// Takes over the terminal: installs the panic-restore hook, enables raw mode,
    /// switches to the alternate screen, and hides the cursor.
    ///
    /// # Errors
    ///
    /// Returns any I/O error from querying the size or switching terminal modes.
    pub fn new() -> io::Result<Self> {
        install_panic_hook();
        terminal::enable_raw_mode()?;
        let mut out = io::stdout();
        out.execute(EnterAlternateScreen)?;
        out.execute(Hide)?;
        let (cols, rows) = terminal::size()?;
        let size = Size::new(cols as i16, rows as i16);
        Ok(Self {
            front: Buffer::new(size),
            size,
        })
    }

    /// Reacts to a terminal resize: adopts the new size, drops the stale front
    /// buffer so the next present is a full repaint, and clears the screen so no
    /// stale glyphs linger.
    fn on_resize(&mut self, size: Size) -> io::Result<()> {
        self.size = size;
        self.front = Buffer::new(size);
        let mut out = io::stdout();
        out.queue(Clear(ClearType::All))?;
        out.flush()
    }
}

impl Backend for CrosstermBackend {
    fn size(&self) -> Size {
        self.size
    }

    fn present(&mut self, frame: &Buffer) -> io::Result<()> {
        let mut out = io::stdout().lock();
        for (p, cell) in frame.diff(&self.front) {
            // A wide grapheme's continuation cell (width 0) is covered by the
            // grapheme to its left; never emit it on its own.
            if cell.width() == 0 {
                continue;
            }
            let style = cell.style();
            out.queue(MoveTo(p.x as u16, p.y as u16))?;
            // Reset first so each cell fully specifies its own appearance.
            out.queue(SetAttribute(Attribute::Reset))?;
            out.queue(SetForegroundColor(to_ct_color(style.fg)))?;
            out.queue(SetBackgroundColor(to_ct_color(style.bg)))?;
            queue_attrs(&mut out, style.attrs)?;
            out.queue(Print(cell.grapheme()))?;
        }
        out.flush()?;
        self.front = frame.clone();
        Ok(())
    }
}

impl EventSource for CrosstermBackend {
    fn poll_event(&mut self, timeout: Duration) -> io::Result<Option<Event>> {
        if !ct::poll(timeout)? {
            return Ok(None);
        }
        match map_event(ct::read()?) {
            Some(Event::Resize(size)) => {
                self.on_resize(size)?;
                Ok(Some(Event::Resize(size)))
            }
            // Unmapped events (focus, paste, key release) read as a quiet tick.
            other => Ok(other),
        }
    }
}

impl Drop for CrosstermBackend {
    fn drop(&mut self) {
        restore_terminal();
    }
}

/// Best-effort, idempotent restoration of the terminal to its pre-`new` state.
/// Safe to call more than once (Drop and the panic hook may both fire).
fn restore_terminal() {
    let mut out = io::stdout();
    let _ = out.execute(Show);
    let _ = out.execute(LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
}

/// Installs (once) a panic hook that restores the terminal *before* delegating to
/// the previous hook, so the panic message prints in a sane, cooked terminal.
fn install_panic_hook() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_terminal();
            previous(info);
        }));
    });
}

/// Translates a raw crossterm event into our backend-agnostic [`Event`], or
/// `None` for events we do not model (focus changes, bracketed paste, key
/// releases). Pure — this is the unit-tested core of the backend.
fn map_event(event: ct::Event) -> Option<Event> {
    match event {
        ct::Event::Key(key) => map_key(key).map(Event::Key),
        ct::Event::Mouse(mouse) => map_mouse(mouse).map(Event::Mouse),
        ct::Event::Resize(cols, rows) => Some(Event::Resize(Size::new(cols as i16, rows as i16))),
        ct::Event::FocusGained | ct::Event::FocusLost | ct::Event::Paste(_) => None,
    }
}

fn map_key(key: ct::KeyEvent) -> Option<KeyEvent> {
    // Ignore key *releases*: terminals with the kitty/Win32 protocols report them,
    // and we want one logical event per press (repeats included).
    if !matches!(key.kind, ct::KeyEventKind::Press | ct::KeyEventKind::Repeat) {
        return None;
    }
    Some(KeyEvent::new(
        map_key_code(key.code)?,
        map_modifiers(key.modifiers),
    ))
}

fn map_key_code(code: ct::KeyCode) -> Option<KeyCode> {
    use ct::KeyCode as C;
    Some(match code {
        C::Char(c) => KeyCode::Char(c),
        C::Enter => KeyCode::Enter,
        C::Esc => KeyCode::Esc,
        C::Backspace => KeyCode::Backspace,
        C::Tab => KeyCode::Tab,
        C::BackTab => KeyCode::BackTab,
        C::Delete => KeyCode::Delete,
        C::Insert => KeyCode::Insert,
        C::Left => KeyCode::Left,
        C::Right => KeyCode::Right,
        C::Up => KeyCode::Up,
        C::Down => KeyCode::Down,
        C::Home => KeyCode::Home,
        C::End => KeyCode::End,
        C::PageUp => KeyCode::PageUp,
        C::PageDown => KeyCode::PageDown,
        C::F(n) => KeyCode::F(n),
        _ => return None,
    })
}

fn map_modifiers(mods: ct::KeyModifiers) -> Modifiers {
    let mut out = Modifiers::NONE;
    if mods.contains(ct::KeyModifiers::SHIFT) {
        out = out | Modifiers::SHIFT;
    }
    if mods.contains(ct::KeyModifiers::CONTROL) {
        out = out | Modifiers::CONTROL;
    }
    if mods.contains(ct::KeyModifiers::ALT) {
        out = out | Modifiers::ALT;
    }
    out
}

fn map_mouse(mouse: ct::MouseEvent) -> Option<MouseEvent> {
    use ct::MouseEventKind as K;
    let kind = match mouse.kind {
        K::Down(b) => MouseKind::Down(map_button(b)),
        K::Up(b) => MouseKind::Up(map_button(b)),
        K::Drag(b) => MouseKind::Drag(map_button(b)),
        K::Moved => MouseKind::Moved,
        K::ScrollUp => MouseKind::ScrollUp,
        K::ScrollDown => MouseKind::ScrollDown,
        // Horizontal scroll has no editor meaning yet.
        K::ScrollLeft | K::ScrollRight => return None,
    };
    Some(MouseEvent {
        kind,
        pos: Point::new(mouse.column as i16, mouse.row as i16),
        modifiers: map_modifiers(mouse.modifiers),
    })
}

fn map_button(button: ct::MouseButton) -> MouseButton {
    match button {
        ct::MouseButton::Left => MouseButton::Left,
        ct::MouseButton::Right => MouseButton::Right,
        ct::MouseButton::Middle => MouseButton::Middle,
    }
}

/// Maps one of our colours to crossterm's. The 16 CGA names map to crossterm's
/// named palette so the user's terminal theme applies; [`Color::Default`] defers
/// to the terminal, and RGB passes straight through (ADR 0005).
fn to_ct_color(color: Color) -> CtColor {
    match color {
        Color::Default => CtColor::Reset,
        Color::Rgb(r, g, b) => CtColor::Rgb { r, g, b },
        Color::Named(named) => match named {
            Color16::Black => CtColor::Black,
            Color16::Blue => CtColor::DarkBlue,
            Color16::Green => CtColor::DarkGreen,
            Color16::Cyan => CtColor::DarkCyan,
            Color16::Red => CtColor::DarkRed,
            Color16::Magenta => CtColor::DarkMagenta,
            Color16::Brown => CtColor::DarkYellow,
            Color16::LightGray => CtColor::Grey,
            Color16::DarkGray => CtColor::DarkGrey,
            Color16::LightBlue => CtColor::Blue,
            Color16::LightGreen => CtColor::Green,
            Color16::LightCyan => CtColor::Cyan,
            Color16::LightRed => CtColor::Red,
            Color16::LightMagenta => CtColor::Magenta,
            Color16::Yellow => CtColor::Yellow,
            Color16::White => CtColor::White,
        },
    }
}

fn queue_attrs(out: &mut impl Write, attrs: Attributes) -> io::Result<()> {
    if attrs.contains(Attributes::BOLD) {
        out.queue(SetAttribute(Attribute::Bold))?;
    }
    if attrs.contains(Attributes::DIM) {
        out.queue(SetAttribute(Attribute::Dim))?;
    }
    if attrs.contains(Attributes::ITALIC) {
        out.queue(SetAttribute(Attribute::Italic))?;
    }
    if attrs.contains(Attributes::UNDERLINE) {
        out.queue(SetAttribute(Attribute::Underlined))?;
    }
    if attrs.contains(Attributes::REVERSE) {
        out.queue(SetAttribute(Attribute::Reverse))?;
    }
    if attrs.contains(Attributes::BLINK) {
        out.queue(SetAttribute(Attribute::SlowBlink))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tracer bullet: Ctrl-Q survives the trip across the seam as our own type.
    #[test]
    fn maps_ctrl_q_key() {
        let raw = ct::Event::Key(ct::KeyEvent::new(
            ct::KeyCode::Char('q'),
            ct::KeyModifiers::CONTROL,
        ));
        assert_eq!(
            map_event(raw),
            Some(Event::Key(KeyEvent::new(
                KeyCode::Char('q'),
                Modifiers::CONTROL
            )))
        );
    }

    #[test]
    fn maps_named_and_function_keys() {
        let enter = ct::Event::Key(ct::KeyEvent::new(
            ct::KeyCode::Enter,
            ct::KeyModifiers::NONE,
        ));
        assert_eq!(
            map_event(enter),
            Some(Event::Key(KeyEvent::new(KeyCode::Enter, Modifiers::NONE)))
        );

        let up = ct::Event::Key(ct::KeyEvent::new(ct::KeyCode::Up, ct::KeyModifiers::NONE));
        assert_eq!(
            map_event(up),
            Some(Event::Key(KeyEvent::new(KeyCode::Up, Modifiers::NONE)))
        );

        let f3 = ct::Event::Key(ct::KeyEvent::new(
            ct::KeyCode::F(3),
            ct::KeyModifiers::SHIFT,
        ));
        assert_eq!(
            map_event(f3),
            Some(Event::Key(KeyEvent::new(KeyCode::F(3), Modifiers::SHIFT)))
        );
    }

    #[test]
    fn maps_resize_to_size() {
        assert_eq!(
            map_event(ct::Event::Resize(80, 25)),
            Some(Event::Resize(Size::new(80, 25)))
        );
    }

    #[test]
    fn maps_mouse_down_with_position() {
        let raw = ct::Event::Mouse(ct::MouseEvent {
            kind: ct::MouseEventKind::Down(ct::MouseButton::Left),
            column: 3,
            row: 4,
            modifiers: ct::KeyModifiers::NONE,
        });
        assert_eq!(
            map_event(raw),
            Some(Event::Mouse(MouseEvent {
                kind: MouseKind::Down(MouseButton::Left),
                pos: Point::new(3, 4),
                modifiers: Modifiers::NONE,
            }))
        );
    }

    #[test]
    fn drops_unmodelled_events() {
        // Focus changes and pastes are not modelled.
        assert_eq!(map_event(ct::Event::FocusGained), None);
        assert_eq!(map_event(ct::Event::Paste("hi".into())), None);

        // Key *releases* are dropped so a press/release pair is one logical event.
        let release = ct::Event::Key(ct::KeyEvent::new_with_kind(
            ct::KeyCode::Char('a'),
            ct::KeyModifiers::NONE,
            ct::KeyEventKind::Release,
        ));
        assert_eq!(map_event(release), None);
    }

    #[test]
    fn cga_palette_maps_to_named_crossterm_colours() {
        assert_eq!(to_ct_color(Color::Default), CtColor::Reset);
        assert_eq!(to_ct_color(Color::Named(Color16::Blue)), CtColor::DarkBlue);
        assert_eq!(to_ct_color(Color::Named(Color16::LightBlue)), CtColor::Blue);
        assert_eq!(to_ct_color(Color::Named(Color16::White)), CtColor::White);
        assert_eq!(
            to_ct_color(Color::Rgb(1, 2, 3)),
            CtColor::Rgb { r: 1, g: 2, b: 3 }
        );
    }
}
