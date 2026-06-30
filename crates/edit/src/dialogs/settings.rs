//! The Settings dialog: editable tab width and recent-files length, with OK,
//! Reset to defaults, and Cancel (ADR 0025).
//!
//! Only the two preferences a user is likely to want a knob for are shown; the
//! Find options and other state stay transparent (persisted automatically). The
//! **Reset to defaults** button restores *all* preferences — including those
//! transparent ones — to their defaults, which is why it is a distinct ending
//! command ([`CM_DEFAULTS`]) the driver acts on rather than just refilling fields.

use rvision::canvas::Canvas;
use rvision::cell::Cell;
use rvision::color::Style;
use rvision::command::{CM_CANCEL, CM_OK, CM_USER, Command};
use rvision::event::{Event, EventResult, KeyCode, MouseButton, MouseEvent, MouseKind};
use rvision::geometry::{Point, Rect, Size};
use rvision::theme::{Role, Theme};
use rvision::view::{Context, Modal, View};
use rvision::widgets::{Button, InputLine};

use crate::settings::Settings;

/// "Reset to defaults" — restore every preference to its default. A bespoke
/// ending command (not `CM_OK`/`CM_CANCEL`) so the driver can tell it apart and
/// reset the transparent settings too. Numbered clear of the app's own ids.
pub const CM_DEFAULTS: Command = Command(CM_USER + 60);

const FOCUS_TAB: usize = 0;
const FOCUS_MRU: usize = 1;
const FOCUS_OK: usize = 2;
const FOCUS_DEFAULTS: usize = 3;
const FOCUS_CANCEL: usize = 4;
const FOCUS_COUNT: usize = 5;

/// A modal Settings prompt, seeded from the current [`Settings`]. After
/// `exec_view` returns `CM_OK`, read [`tab_width`](SettingsDialog::tab_width) and
/// [`recent_limit`](SettingsDialog::recent_limit); on [`CM_DEFAULTS`] the driver
/// resets to defaults instead.
pub struct SettingsDialog {
    size: Size,
    style: Style,
    tab: InputLine,
    mru: InputLine,
    ok: Button,
    defaults: Button,
    cancel: Button,
    focus: usize,
}

impl SettingsDialog {
    /// Builds the dialog, pre-filling the fields with the current settings (the
    /// user backspaces to edit — `InputLine` has no select-all).
    pub fn new(theme: &Theme, current: &Settings) -> Self {
        let size = Size::new(50, 11);
        let iw = size.width - 2;
        let ih = size.height - 2;
        let mut dialog = Self {
            size,
            style: theme.style(Role::DialogBackground),
            tab: InputLine::new(rect(0, 1, 8, 1), theme).with_text(&current.tab_width.to_string()),
            mru: InputLine::new(rect(0, 4, 8, 1), theme)
                .with_text(&current.recent_limit.to_string()),
            ok: Button::new(rect(2, ih - 1, 10, 1), "OK", CM_OK, theme).default(true),
            defaults: Button::new(rect(14, ih - 1, 16, 1), "Defaults", CM_DEFAULTS, theme),
            cancel: Button::new(rect(iw - 12, ih - 1, 10, 1), "Cancel", CM_CANCEL, theme),
            focus: FOCUS_TAB,
        };
        dialog.apply_focus();
        dialog
    }

    /// The entered tab width, or `None` if the field is empty or not a number (the
    /// driver then keeps the current value). Range-clamped on apply.
    pub fn tab_width(&self) -> Option<usize> {
        self.tab.text().trim().parse::<usize>().ok()
    }

    /// The entered recent-files length, or `None` if empty/non-numeric.
    pub fn recent_limit(&self) -> Option<usize> {
        self.mru.text().trim().parse::<usize>().ok()
    }

    /// Pushes the focus flag to whichever control now holds it (ADR 0017).
    fn apply_focus(&mut self) {
        self.tab.set_focused(self.focus == FOCUS_TAB);
        self.mru.set_focused(self.focus == FOCUS_MRU);
        self.ok.set_focused(self.focus == FOCUS_OK);
        self.defaults.set_focused(self.focus == FOCUS_DEFAULTS);
        self.cancel.set_focused(self.focus == FOCUS_CANCEL);
    }

    /// Moves focus `delta` steps round the five controls.
    fn move_focus(&mut self, delta: isize) {
        let n = FOCUS_COUNT as isize;
        self.focus = (((self.focus as isize + delta) % n + n) % n) as usize;
        self.apply_focus();
    }

    /// `Enter`: the focused button's action, or the default (OK) from a field.
    fn on_enter(&self, ctx: &mut Context) -> EventResult {
        let command = match self.focus {
            FOCUS_DEFAULTS => CM_DEFAULTS,
            FOCUS_CANCEL => CM_CANCEL,
            _ => CM_OK,
        };
        ctx.post(command);
        EventResult::Consumed
    }

    /// The interior rectangle (inset one cell on every side), dialog-local.
    fn interior(&self) -> Rect {
        Rect::from_origin_size(
            Point::new(1, 1),
            Size::new((self.size.width - 2).max(0), (self.size.height - 2).max(0)),
        )
    }

    /// The controls in focus order, as `&dyn View` for drawing/hit-testing.
    fn controls(&self) -> [&dyn View; FOCUS_COUNT] {
        [&self.tab, &self.mru, &self.ok, &self.defaults, &self.cancel]
    }

    /// Routes a key to the focused control by index.
    fn route_key(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        match self.focus {
            FOCUS_TAB => self.tab.handle_event(event, ctx),
            FOCUS_MRU => self.mru.handle_event(event, ctx),
            FOCUS_OK => self.ok.handle_event(event, ctx),
            FOCUS_DEFAULTS => self.defaults.handle_event(event, ctx),
            FOCUS_CANCEL => self.cancel.handle_event(event, ctx),
            _ => EventResult::Ignored,
        }
    }

    /// Routes a mouse event (dialog-local) to the control under the pointer,
    /// focusing it on a press — mirroring the key dispatch.
    fn handle_mouse(&mut self, m: &MouseEvent, ctx: &mut Context) -> EventResult {
        let io = self.interior().origin();
        let p = m.pos.offset(-io.x, -io.y);
        let bounds: Vec<Rect> = self.controls().iter().map(|c| c.bounds()).collect();
        let Some(i) = bounds.iter().position(|b| b.contains(p)) else {
            return EventResult::Ignored;
        };
        if matches!(m.kind, MouseKind::Down(MouseButton::Left)) {
            self.focus = i;
            self.apply_focus();
        }
        let b = bounds[i];
        let local = Event::Mouse(MouseEvent {
            pos: p.offset(-b.origin().x, -b.origin().y),
            ..*m
        });
        self.route_key(&local, ctx)
    }
}

fn rect(x: i16, y: i16, w: i16, h: i16) -> Rect {
    Rect::from_origin_size(Point::new(x, y), Size::new(w, h))
}

impl View for SettingsDialog {
    fn bounds(&self) -> Rect {
        Rect::from_origin_size(Point::new(0, 0), self.size)
    }

    fn draw(&self, canvas: &mut Canvas) {
        let area = canvas.bounds();
        canvas.fill(area, &Cell::blank(self.style));
        canvas.draw_box(area, self.style);
        let title = " Settings ";
        let x = ((area.width() - title.chars().count() as i16) / 2).max(1);
        canvas.put_str(Point::new(x, 0), title, self.style);

        let interior = self.interior();
        if interior.is_empty() {
            return;
        }
        let mut sub = canvas.child(interior);
        sub.put_str(Point::new(0, 0), "Tab width (1-16):", self.style);
        sub.put_str(Point::new(0, 3), "Recent files shown (0-9):", self.style);
        for control in self.controls() {
            let mut child = sub.child(control.bounds());
            control.draw(&mut child);
        }
    }

    fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => key,
            Event::Mouse(m) => return self.handle_mouse(m, ctx),
            _ => return EventResult::Ignored,
        };
        match key.code {
            KeyCode::Esc => {
                ctx.post(CM_CANCEL);
                EventResult::Consumed
            }
            KeyCode::Tab => {
                self.move_focus(1);
                EventResult::Consumed
            }
            KeyCode::BackTab => {
                self.move_focus(-1);
                EventResult::Consumed
            }
            KeyCode::Enter => self.on_enter(ctx),
            _ => self.route_key(event, ctx),
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}

impl Modal for SettingsDialog {
    fn size(&self) -> Size {
        self.size
    }

    fn ends_on(&self, command: Command) -> bool {
        command == CM_OK || command == CM_CANCEL || command == CM_DEFAULTS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rvision::command::CommandSet;
    use rvision::event::{KeyEvent, Modifiers};

    fn dialog() -> SettingsDialog {
        let current = Settings {
            tab_width: 4,
            recent_limit: 5,
            ..Settings::default()
        };
        SettingsDialog::new(&Theme::default(), &current)
    }

    fn press(d: &mut SettingsDialog, code: KeyCode) -> Vec<Event> {
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        d.handle_event(&Event::Key(KeyEvent::new(code, Modifiers::NONE)), &mut ctx);
        ctx.take_posted()
    }

    #[test]
    fn fields_are_seeded_from_the_current_settings() {
        let d = dialog();
        assert_eq!(d.tab_width(), Some(4));
        assert_eq!(d.recent_limit(), Some(5));
    }

    #[test]
    fn editing_a_field_reads_the_new_value() {
        let mut d = dialog();
        press(&mut d, KeyCode::Backspace); // clear the seeded "4"
        press(&mut d, KeyCode::Char('2'));
        assert_eq!(d.tab_width(), Some(2));
    }

    #[test]
    fn non_numeric_field_is_none() {
        let mut d = dialog();
        press(&mut d, KeyCode::Backspace);
        press(&mut d, KeyCode::Char('x'));
        assert_eq!(d.tab_width(), None);
    }

    #[test]
    fn tab_cycles_all_five_controls() {
        let mut d = dialog();
        assert_eq!(d.focus, FOCUS_TAB);
        for expected in [FOCUS_MRU, FOCUS_OK, FOCUS_DEFAULTS, FOCUS_CANCEL, FOCUS_TAB] {
            press(&mut d, KeyCode::Tab);
            assert_eq!(d.focus, expected);
        }
    }

    #[test]
    fn enter_on_defaults_posts_the_reset_command() {
        let mut d = dialog();
        for _ in 0..FOCUS_DEFAULTS {
            press(&mut d, KeyCode::Tab);
        }
        assert_eq!(d.focus, FOCUS_DEFAULTS);
        assert_eq!(
            press(&mut d, KeyCode::Enter),
            vec![Event::Command(CM_DEFAULTS)]
        );
    }

    #[test]
    fn enter_from_a_field_accepts_with_ok() {
        let mut d = dialog();
        assert_eq!(press(&mut d, KeyCode::Enter), vec![Event::Command(CM_OK)]);
    }

    #[test]
    fn esc_cancels() {
        let mut d = dialog();
        assert_eq!(press(&mut d, KeyCode::Esc), vec![Event::Command(CM_CANCEL)]);
    }

    #[test]
    fn ends_on_ok_cancel_and_defaults() {
        let d = dialog();
        assert!(Modal::ends_on(&d, CM_OK));
        assert!(Modal::ends_on(&d, CM_CANCEL));
        assert!(Modal::ends_on(&d, CM_DEFAULTS));
        assert!(!Modal::ends_on(&d, Command(CM_USER + 1)));
    }

    fn click(d: &mut SettingsDialog, x: i16, y: i16) -> Vec<Event> {
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        d.handle_event(
            &Event::Mouse(MouseEvent {
                kind: MouseKind::Down(MouseButton::Left),
                pos: Point::new(x, y),
                modifiers: Modifiers::NONE,
            }),
            &mut ctx,
        );
        ctx.take_posted()
    }

    #[test]
    fn clicking_defaults_focuses_and_posts_reset() {
        let mut d = dialog();
        // Defaults sits at interior-local (14, 8) → dialog-local (15, 9).
        let posted = click(&mut d, 16, 9);
        assert_eq!(d.focus, FOCUS_DEFAULTS);
        assert_eq!(posted, vec![Event::Command(CM_DEFAULTS)]);
    }
}
