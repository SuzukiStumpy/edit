//! The menu bar and its pull-downs (TurboVision's `TMenuBar` / `TMenuBox`).
//!
//! The bar shows menu titles across the top row; opening one drops a pull-down
//! listing its items. There is no modal loop yet (that is Phase 5's `exec_view`):
//! the open/highlight state lives on the [`MenuBar`] and the application shell
//! drives it — feeding it keys first (so it can claim `Alt`-hot-keys and, while
//! open, run modally) and drawing its pull-down last, as an overlay over the whole
//! frame (ADR 0016).

use crate::canvas::Canvas;
use crate::cell::Cell;
use crate::color::Style;
use crate::command::{Command, CommandSet};
use crate::event::{Event, EventResult, KeyCode, Modifiers, MouseButton, MouseEvent, MouseKind};
use crate::geometry::{Point, Rect, Size};
use crate::theme::{Role, Theme};
use crate::view::{Context, View};

/// One entry in a pull-down menu: a label, the command it posts, and an optional
/// shortcut shown right-aligned (the accelerator itself is the status line's /
/// app's job; this is only the reminder text).
pub struct MenuItem {
    label: String,
    command: Command,
    shortcut: Option<String>,
}

impl MenuItem {
    /// Creates an item labelled `label` that posts `command` when chosen.
    pub fn new(label: &str, command: Command) -> Self {
        Self {
            label: label.to_string(),
            command,
            shortcut: None,
        }
    }

    /// Adds the right-aligned shortcut reminder (e.g. `"Ctrl-N"`).
    pub fn with_shortcut(mut self, shortcut: &str) -> Self {
        self.shortcut = Some(shortcut.to_string());
        self
    }
}

/// One pull-down: a title and its items. The title's first character is its
/// `Alt`-hot-key (case-insensitive).
pub struct Menu {
    title: String,
    items: Vec<MenuItem>,
}

impl Menu {
    /// Creates a menu titled `title` listing `items`.
    pub fn new(title: &str, items: Vec<MenuItem>) -> Self {
        Self {
            title: title.to_string(),
            items,
        }
    }

    /// The `Alt`-hot-key that opens this menu: its title's first letter, lowercased.
    fn hotkey(&self) -> Option<char> {
        self.title.chars().next().map(|c| c.to_ascii_lowercase())
    }
}

/// The top-row menu bar.
pub struct MenuBar {
    bounds: Rect,
    menus: Vec<Menu>,
    open: Option<usize>,
    highlight: usize,
    bar_style: Style,
    selected_style: Style,
    disabled_style: Style,
    /// Which commands are live, pushed in before a draw so disabled items can grey
    /// themselves (the same state-in-draw "push" as `View::set_focused`). Empty by
    /// default, so every item is enabled until the app says otherwise.
    commands: CommandSet,
}

impl MenuBar {
    /// Creates a menu bar at `bounds` from `menus`, taking its colours from
    /// `theme` ([`Role::MenuBar`], [`Role::MenuSelected`]).
    pub fn new(bounds: Rect, menus: Vec<Menu>, theme: &Theme) -> Self {
        Self {
            bounds,
            menus,
            open: None,
            highlight: 0,
            bar_style: theme.style(Role::MenuBar),
            selected_style: theme.style(Role::MenuSelected),
            disabled_style: theme.style(Role::MenuDisabled),
            commands: CommandSet::new(),
        }
    }

    /// Pushes the current command-enabled state in before a draw, so a pull-down
    /// can grey the items whose command is disabled (ADR 0003/0004). Call it from
    /// the same place the app keeps its [`CommandSet`] up to date; dispatch already
    /// gates disabled commands, so this is purely the visual half.
    pub fn sync_enabled(&mut self, commands: &CommandSet) {
        self.commands = commands.clone();
    }

    /// Repositions the bar (the shell calls this as the terminal resizes).
    pub fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }

    /// Whether a pull-down is currently open (the shell routes all keys here while
    /// it is, ADR 0016).
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Closes any open pull-down.
    pub fn close(&mut self) {
        self.open = None;
        self.highlight = 0;
    }

    /// The starting column of each menu title's text, laid out left to right with
    /// one space of padding on each side.
    fn title_starts(&self) -> Vec<i16> {
        let mut xs = Vec::with_capacity(self.menus.len());
        let mut x = 0;
        for menu in &self.menus {
            x += 1;
            xs.push(x);
            x += menu.title.chars().count() as i16 + 1;
        }
        xs
    }

    /// The menu whose title (with its one-space padding) sits under bar column `x`,
    /// or `None` for an empty stretch of the bar. Matches the highlighted ` Title `
    /// region drawn by [`draw`](Self::draw).
    fn title_at(&self, x: i16) -> Option<usize> {
        let starts = self.title_starts();
        self.menus.iter().enumerate().find_map(|(i, menu)| {
            let start = starts[i];
            let len = menu.title.chars().count() as i16;
            (x >= start - 1 && x <= start + len).then_some(i)
        })
    }

    /// The item under `pos` (screen coordinates) when a pull-down is open, or `None`
    /// if `pos` is outside the open box. Uses the same [`pulldown_area`](Self::pulldown_area)
    /// the overlay draws, so a click lands on exactly the row it looks like.
    fn item_at(&self, pos: Point) -> Option<usize> {
        let (index, menu) = self.open_menu_ref()?;
        if menu.items.is_empty() {
            return None;
        }
        let area = self.pulldown_area(index, menu, self.bounds.width());
        let left = area.origin().x;
        let box_w = area.width();
        // Interior only: exclude the border columns and the top/bottom border rows.
        if pos.x <= left || pos.x >= left + box_w - 1 {
            return None;
        }
        let first_row = area.origin().y + 1; // box top + 1 border row
        let row = pos.y - first_row;
        (row >= 0 && (row as usize) < menu.items.len()).then_some(row as usize)
    }

    /// Opens menu `index` (clamped), resetting the highlight to its first item.
    fn open_menu(&mut self, index: usize) {
        if index < self.menus.len() {
            self.open = Some(index);
            self.highlight = 0;
        }
    }

    /// The currently open menu, if any.
    fn open_menu_ref(&self) -> Option<(usize, &Menu)> {
        self.open.map(|i| (i, &self.menus[i]))
    }

    /// Runs the modal key handling while a menu is open: arrows move, `Enter`
    /// chooses, `Esc` closes; every other key is swallowed so nothing leaks to the
    /// editor underneath. Returns the result (always `Consumed` while open).
    fn handle_open(&mut self, code: KeyCode, ctx: &mut Context) -> EventResult {
        let n = self.menus.len();
        let open = self.open.expect("handle_open called while closed");
        let items = self.menus[open].items.len();
        match code {
            KeyCode::Esc => self.close(),
            KeyCode::Left if n > 0 => self.open_menu((open + n - 1) % n),
            KeyCode::Right if n > 0 => self.open_menu((open + 1) % n),
            KeyCode::Up if items > 0 => self.highlight = (self.highlight + items - 1) % items,
            KeyCode::Down if items > 0 => self.highlight = (self.highlight + 1) % items,
            KeyCode::Enter if items > 0 => {
                let command = self.menus[open].items[self.highlight].command;
                self.close();
                // Gated by Context: a disabled item posts nothing (ADR 0003).
                ctx.post(command);
            }
            _ => {}
        }
        EventResult::Consumed
    }

    /// Tries to open a menu from a closed bar: `Alt`+a title's first letter, or
    /// `F10` for the first menu. Returns whether it claimed the key.
    fn handle_closed(&mut self, code: KeyCode, modifiers: Modifiers) -> EventResult {
        match code {
            KeyCode::F(10) if !self.menus.is_empty() => {
                self.open_menu(0);
                EventResult::Consumed
            }
            KeyCode::Char(c) if modifiers.contains(Modifiers::ALT) => {
                let c = c.to_ascii_lowercase();
                if let Some(index) = self.menus.iter().position(|m| m.hotkey() == Some(c)) {
                    self.open_menu(index);
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    /// The pull-down box (border included) for the open menu `index`, in the same
    /// screen coordinates the overlay draws in: anchored a row below the bar, its
    /// left edge under the title but pulled back so the box stays on screen. The
    /// single source of the box geometry, shared by [`draw_overlay`](Self::draw_overlay)
    /// and mouse hit-testing.
    fn pulldown_area(&self, index: usize, menu: &Menu, screen_w: i16) -> Rect {
        let starts = self.title_starts();
        let box_w = self.pulldown_width(menu);
        let box_h = menu.items.len() as i16 + 2;
        let left = (starts[index] - 1).min(screen_w - box_w).max(0);
        Rect::from_origin_size(Point::new(left, 1), Size::new(box_w, box_h))
    }

    /// Routes a mouse event (positions in screen coordinates, the same space the
    /// bar and its overlay draw in — the bar sits on row `bounds.y`, the pull-down
    /// the rows below). Clicking a title opens it (or toggles it shut); clicking a
    /// pull-down item chooses it; clicking anywhere else dismisses an open menu.
    /// While open, moving over a title or item tracks the highlight (TV feel).
    fn handle_mouse(&mut self, mouse: &MouseEvent, ctx: &mut Context) -> EventResult {
        let on_bar = mouse.pos.y == self.bounds.origin().y;
        match mouse.kind {
            MouseKind::Down(MouseButton::Left) => {
                if on_bar {
                    return match self.title_at(mouse.pos.x) {
                        Some(i) if self.open == Some(i) => {
                            self.close(); // a second click on the open title shuts it
                            EventResult::Consumed
                        }
                        Some(i) => {
                            self.open_menu(i);
                            EventResult::Consumed
                        }
                        // Bare stretch of the bar: dismiss any open pull-down.
                        None if self.is_open() => {
                            self.close();
                            EventResult::Consumed
                        }
                        None => EventResult::Ignored,
                    };
                }
                if self.is_open() {
                    if let Some(item) = self.item_at(mouse.pos) {
                        let command = self.menus[self.open.unwrap()].items[item].command;
                        self.close();
                        ctx.post(command); // Context-gated: a disabled item posts nothing
                    } else {
                        self.close(); // click off the box dismisses it
                    }
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            MouseKind::Moved | MouseKind::Drag(MouseButton::Left) if self.is_open() => {
                if let Some(item) = self.item_at(mouse.pos) {
                    self.highlight = item;
                    EventResult::Consumed
                } else if on_bar {
                    if let Some(i) = self.title_at(mouse.pos.x) {
                        if self.open != Some(i) {
                            self.open_menu(i); // slide across the bar with the button down
                        }
                        return EventResult::Consumed;
                    }
                    EventResult::Ignored
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    /// Draws an open pull-down over the whole frame. The shell calls this after
    /// everything else so the box sits on top (ADR 0016); a closed bar draws
    /// nothing.
    pub fn draw_overlay(&self, canvas: &mut Canvas) {
        let Some((index, menu)) = self.open_menu_ref() else {
            return;
        };
        if menu.items.is_empty() {
            return;
        }
        let area = self.pulldown_area(index, menu, canvas.size().width);
        let left = area.origin().x;
        let box_w = area.width();

        canvas.fill(area, &Cell::blank(self.bar_style));
        canvas.draw_box(area, self.bar_style);
        for (i, item) in menu.items.iter().enumerate() {
            let row = 2 + i as i16;
            // A disabled item can't light up: it stays greyed even on the highlight
            // row, so the whole line (fill included) reads as unavailable.
            let style = if !self.commands.is_enabled(item.command) {
                self.disabled_style
            } else if i == self.highlight {
                self.selected_style
            } else {
                self.bar_style
            };
            // Repaint the interior row so the highlight is a full-width bar.
            let inner = Rect::from_origin_size(Point::new(left + 1, row), Size::new(box_w - 2, 1));
            canvas.fill(inner, &Cell::blank(style));
            canvas.put_str(Point::new(left + 2, row), &item.label, style);
            if let Some(shortcut) = &item.shortcut {
                let sx = left + box_w - 2 - shortcut.chars().count() as i16;
                canvas.put_str(Point::new(sx, row), shortcut, style);
            }
        }
    }

    /// The pull-down box width: widest "` label  shortcut `" line, plus borders.
    fn pulldown_width(&self, menu: &Menu) -> i16 {
        let label_w = menu
            .items
            .iter()
            .map(|it| it.label.chars().count())
            .max()
            .unwrap_or(0);
        let short_w = menu
            .items
            .iter()
            .filter_map(|it| it.shortcut.as_ref().map(|s| s.chars().count()))
            .max()
            .unwrap_or(0);
        let gap = if short_w > 0 { short_w + 2 } else { 0 };
        // 1 leading + label + gap + 1 trailing, then +2 for the borders.
        (1 + label_w + gap + 1 + 2) as i16
    }
}

impl View for MenuBar {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn draw(&self, canvas: &mut Canvas) {
        let area = canvas.bounds();
        canvas.fill(area, &Cell::blank(self.bar_style));
        let starts = self.title_starts();
        for (i, menu) in self.menus.iter().enumerate() {
            let start = starts[i];
            if self.open == Some(i) {
                // Highlight the open title together with its surrounding spaces.
                let label = format!(" {} ", menu.title);
                canvas.put_str(Point::new(start - 1, 0), &label, self.selected_style);
            } else {
                canvas.put_str(Point::new(start, 0), &menu.title, self.bar_style);
            }
        }
    }

    fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        match event {
            Event::Key(key) if self.is_open() => self.handle_open(key.code, ctx),
            Event::Key(key) => self.handle_closed(key.code, key.modifiers),
            Event::Mouse(mouse) => self.handle_mouse(mouse, ctx),
            _ => EventResult::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::command::{CM_USER, CommandSet};
    use crate::event::KeyEvent;

    const CM_NEW: Command = Command(CM_USER + 1);
    const CM_OPEN: Command = Command(CM_USER + 2);
    const CM_COPY: Command = Command(CM_USER + 3);

    fn rect(x: i16, y: i16, w: i16, h: i16) -> Rect {
        Rect::from_origin_size(Point::new(x, y), Size::new(w, h))
    }

    fn bar() -> MenuBar {
        MenuBar::new(
            rect(0, 0, 40, 1),
            vec![
                Menu::new(
                    "File",
                    vec![
                        MenuItem::new("New", CM_NEW).with_shortcut("Ctrl-N"),
                        MenuItem::new("Open...", CM_OPEN).with_shortcut("Ctrl-O"),
                    ],
                ),
                Menu::new(
                    "Edit",
                    vec![MenuItem::new("Copy", CM_COPY).with_shortcut("Ctrl-C")],
                ),
            ],
            &Theme::default(),
        )
    }

    fn key(code: KeyCode, mods: Modifiers) -> Event {
        Event::Key(KeyEvent::new(code, mods))
    }

    /// Renders the bar like the shell does: the bar into a one-row sub-canvas at
    /// the top, then the pull-down overlay over the whole frame.
    fn render(bar: &MenuBar, w: i16, h: i16) -> String {
        let mut buf = Buffer::new(Size::new(w, h));
        let mut root = Canvas::new(&mut buf);
        {
            let mut barc = root.child(rect(0, 0, w, 1));
            bar.draw(&mut barc);
        }
        bar.draw_overlay(&mut root);
        buf.to_text()
    }

    // --- Drawing ---

    #[test]
    fn snapshot_closed_bar() {
        insta::assert_snapshot!(render(&bar(), 40, 6));
    }

    #[test]
    fn snapshot_open_file_menu() {
        let mut bar = bar();
        bar.handle_event(
            &key(KeyCode::Char('f'), Modifiers::ALT),
            &mut Context::new(&CommandSet::new()),
        );
        insta::assert_snapshot!(render(&bar, 40, 6));
    }

    // --- Opening / accelerators ---

    #[test]
    fn alt_letter_opens_the_matching_menu() {
        let mut bar = bar();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        assert!(!bar.is_open());
        let r = bar.handle_event(&key(KeyCode::Char('e'), Modifiers::ALT), &mut ctx);
        assert_eq!(r, EventResult::Consumed);
        assert_eq!(bar.open, Some(1)); // Edit
    }

    #[test]
    fn f10_opens_the_first_menu() {
        let mut bar = bar();
        bar.handle_event(
            &key(KeyCode::F(10), Modifiers::NONE),
            &mut Context::new(&CommandSet::new()),
        );
        assert_eq!(bar.open, Some(0));
    }

    #[test]
    fn a_closed_bar_ignores_ordinary_keys() {
        // Plain letters (no Alt) must pass through to the editor.
        let mut bar = bar();
        let r = bar.handle_event(
            &key(KeyCode::Char('f'), Modifiers::NONE),
            &mut Context::new(&CommandSet::new()),
        );
        assert_eq!(r, EventResult::Ignored);
        assert!(!bar.is_open());
    }

    // --- Navigation while open ---

    #[test]
    fn left_right_switch_menus_and_wrap() {
        let mut bar = bar();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        bar.handle_event(&key(KeyCode::F(10), Modifiers::NONE), &mut ctx); // File
        bar.handle_event(&key(KeyCode::Right, Modifiers::NONE), &mut ctx);
        assert_eq!(bar.open, Some(1)); // Edit
        bar.handle_event(&key(KeyCode::Right, Modifiers::NONE), &mut ctx);
        assert_eq!(bar.open, Some(0), "wraps back to File");
        bar.handle_event(&key(KeyCode::Left, Modifiers::NONE), &mut ctx);
        assert_eq!(bar.open, Some(1), "and back the other way");
    }

    #[test]
    fn up_down_move_the_highlight_and_wrap() {
        let mut bar = bar();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        bar.handle_event(&key(KeyCode::F(10), Modifiers::NONE), &mut ctx); // File: 2 items
        assert_eq!(bar.highlight, 0);
        bar.handle_event(&key(KeyCode::Down, Modifiers::NONE), &mut ctx);
        assert_eq!(bar.highlight, 1);
        bar.handle_event(&key(KeyCode::Down, Modifiers::NONE), &mut ctx);
        assert_eq!(bar.highlight, 0, "wraps");
        bar.handle_event(&key(KeyCode::Up, Modifiers::NONE), &mut ctx);
        assert_eq!(bar.highlight, 1, "wraps the other way");
    }

    #[test]
    fn switching_menus_resets_the_highlight() {
        let mut bar = bar();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        bar.handle_event(&key(KeyCode::F(10), Modifiers::NONE), &mut ctx);
        bar.handle_event(&key(KeyCode::Down, Modifiers::NONE), &mut ctx);
        assert_eq!(bar.highlight, 1);
        bar.handle_event(&key(KeyCode::Right, Modifiers::NONE), &mut ctx);
        assert_eq!(bar.highlight, 0);
    }

    // --- Choosing / dismissing ---

    #[test]
    fn enter_posts_the_highlighted_command_and_closes() {
        let mut bar = bar();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        bar.handle_event(&key(KeyCode::F(10), Modifiers::NONE), &mut ctx); // File
        bar.handle_event(&key(KeyCode::Down, Modifiers::NONE), &mut ctx); // Open...
        let r = bar.handle_event(&key(KeyCode::Enter, Modifiers::NONE), &mut ctx);
        assert_eq!(r, EventResult::Consumed);
        assert!(!bar.is_open(), "choosing closes the menu");
        assert_eq!(ctx.posted(), &[Event::Command(CM_OPEN)]);
    }

    #[test]
    fn a_disabled_items_command_is_not_posted() {
        let mut bar = bar();
        let mut cs = CommandSet::new();
        cs.disable(CM_NEW);
        let mut ctx = Context::new(&cs);
        bar.handle_event(&key(KeyCode::F(10), Modifiers::NONE), &mut ctx); // File, item 0 = New
        bar.handle_event(&key(KeyCode::Enter, Modifiers::NONE), &mut ctx);
        assert!(!bar.is_open(), "still closes, like TurboVision");
        assert!(
            ctx.posted().is_empty(),
            "but the disabled command never fires"
        );
    }

    #[test]
    fn a_disabled_item_draws_greyed() {
        // The visual half of disabled commands: an item whose command is disabled
        // draws in Role::MenuDisabled, even when it is the highlighted row (a
        // disabled item can't light up). Enabled items are unaffected.
        let mut bar = bar();
        let mut cs = CommandSet::new();
        cs.disable(CM_NEW); // File item 0 = New (also the row highlighted on open)
        bar.sync_enabled(&cs);
        bar.handle_event(&key(KeyCode::F(10), Modifiers::NONE), &mut Context::new(&cs));

        let mut buf = Buffer::new(Size::new(40, 6));
        let mut root = Canvas::new(&mut buf);
        bar.draw_overlay(&mut root);

        let theme = Theme::default();
        // New on row 2 is disabled -> greyed despite being highlighted.
        assert_eq!(
            buf.get(Point::new(2, 2)).unwrap().style(),
            theme.style(Role::MenuDisabled)
        );
        // Open... on row 3 is enabled -> ordinary bar style.
        assert_eq!(
            buf.get(Point::new(2, 3)).unwrap().style(),
            theme.style(Role::MenuBar)
        );
    }

    #[test]
    fn esc_closes_without_posting() {
        let mut bar = bar();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        bar.handle_event(&key(KeyCode::F(10), Modifiers::NONE), &mut ctx);
        let r = bar.handle_event(&key(KeyCode::Esc, Modifiers::NONE), &mut ctx);
        assert_eq!(r, EventResult::Consumed);
        assert!(!bar.is_open());
        assert!(ctx.posted().is_empty());
    }

    #[test]
    fn an_open_menu_swallows_unrelated_keys() {
        // While open the menu is modal: a stray letter is consumed, not leaked.
        let mut bar = bar();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        bar.handle_event(&key(KeyCode::F(10), Modifiers::NONE), &mut ctx);
        let r = bar.handle_event(&key(KeyCode::Char('z'), Modifiers::NONE), &mut ctx);
        assert_eq!(r, EventResult::Consumed);
        assert!(bar.is_open());
    }

    // --- Mouse (Phase 9b) ---
    //
    // Layout of `bar()`: titles `File` (cols 0..=5) and `Edit` (cols 6..=11) on
    // row 0; the File pull-down is a 19-wide box at (0, 1), items `New` on row 2
    // and `Open...` on row 3.

    fn click(x: i16, y: i16) -> Event {
        Event::Mouse(MouseEvent {
            kind: MouseKind::Down(MouseButton::Left),
            pos: Point::new(x, y),
            modifiers: Modifiers::NONE,
        })
    }

    #[test]
    fn clicking_a_title_opens_that_menu() {
        let mut bar = bar();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        let r = bar.handle_event(&click(7, 0), &mut ctx); // the Edit title
        assert_eq!(r, EventResult::Consumed);
        assert_eq!(bar.open, Some(1));
    }

    #[test]
    fn clicking_the_open_title_again_closes_it() {
        let mut bar = bar();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        bar.handle_event(&click(1, 0), &mut ctx); // open File
        assert_eq!(bar.open, Some(0));
        bar.handle_event(&click(1, 0), &mut ctx); // click it again
        assert!(!bar.is_open());
    }

    #[test]
    fn clicking_another_title_switches_menus() {
        let mut bar = bar();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        bar.handle_event(&click(1, 0), &mut ctx); // File
        bar.handle_event(&click(7, 0), &mut ctx); // Edit
        assert_eq!(bar.open, Some(1));
    }

    #[test]
    fn clicking_an_item_posts_its_command_and_closes() {
        let mut bar = bar();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        bar.handle_event(&click(1, 0), &mut ctx); // open File
        let r = bar.handle_event(&click(3, 3), &mut ctx); // the Open... row
        assert_eq!(r, EventResult::Consumed);
        assert!(!bar.is_open());
        assert_eq!(ctx.posted(), &[Event::Command(CM_OPEN)]);
    }

    #[test]
    fn clicking_off_an_open_pulldown_dismisses_it() {
        let mut bar = bar();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        bar.handle_event(&click(1, 0), &mut ctx); // open File
        let r = bar.handle_event(&click(30, 5), &mut ctx); // empty desktop below
        assert_eq!(r, EventResult::Consumed);
        assert!(!bar.is_open());
        assert!(ctx.posted().is_empty());
    }

    #[test]
    fn clicking_a_disabled_item_posts_nothing_but_closes() {
        let mut bar = bar();
        let mut cs = CommandSet::new();
        cs.disable(CM_NEW);
        let mut ctx = Context::new(&cs);
        bar.handle_event(&click(1, 0), &mut ctx); // open File
        bar.handle_event(&click(3, 2), &mut ctx); // the New row (disabled)
        assert!(!bar.is_open());
        assert!(ctx.posted().is_empty());
    }

    #[test]
    fn clicking_a_bare_part_of_a_closed_bar_is_ignored() {
        let mut bar = bar();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        let r = bar.handle_event(&click(20, 0), &mut ctx); // past the last title
        assert_eq!(r, EventResult::Ignored);
        assert!(!bar.is_open());
    }

    #[test]
    fn moving_over_an_item_tracks_the_highlight() {
        let mut bar = bar();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        bar.handle_event(&click(1, 0), &mut ctx); // open File, highlight = 0
        let moved = Event::Mouse(MouseEvent {
            kind: MouseKind::Moved,
            pos: Point::new(3, 3), // hover the Open... row
            modifiers: Modifiers::NONE,
        });
        bar.handle_event(&moved, &mut ctx);
        assert_eq!(bar.highlight, 1);
    }
}
