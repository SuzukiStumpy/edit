//! A scrollable single-selection list (TurboVision's `TListBox`/`TListViewer`).
//!
//! A focusable [control](super) showing one item per row. `Up`/`Down` move the
//! selection by one, `PageUp`/`PageDown` by a screenful, `Home`/`End` to the
//! ends; the view always scrolls to keep the selection visible. When the list is
//! longer than the box a [`ScrollBar`](super::ScrollBar) is drawn down the right
//! edge. `Enter` is left to bubble so the dialog's default button (e.g. *Open*)
//! acts on the selected item.

use crate::canvas::Canvas;
use crate::cell::Cell;
use crate::color::Style;
use crate::event::{Event, EventResult, KeyCode, MouseButton, MouseEvent, MouseKind};
use crate::geometry::{Point, Rect, Size};
use crate::theme::{Role, Theme};
use crate::view::{Context, View};

use super::{ScrollBar, ScrollPart};

/// A scrollable list with one selected row.
pub struct ListBox {
    bounds: Rect,
    items: Vec<String>,
    selected: Option<usize>,
    top: usize,
    focused: bool,
    style: Style,
    focus_style: Style,
}

impl ListBox {
    /// Creates a list at `bounds` over `items`; the first item starts selected
    /// (or nothing, if the list is empty).
    pub fn new(bounds: Rect, items: Vec<String>, theme: &Theme) -> Self {
        let selected = (!items.is_empty()).then_some(0);
        Self {
            bounds,
            items,
            selected,
            top: 0,
            focused: false,
            style: theme.style(Role::Input),
            focus_style: theme.style(Role::Selection),
        }
    }

    /// The index of the selected item, if any.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The text of the selected item, if any.
    pub fn selected_text(&self) -> Option<&str> {
        self.selected
            .and_then(|i| self.items.get(i))
            .map(String::as_str)
    }

    /// The number of fully visible rows.
    fn rows(&self) -> usize {
        self.bounds.height().max(0) as usize
    }

    /// Moves the selection by `delta` rows (clamped), then scrolls it into view.
    fn move_by(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let last = self.items.len() as isize - 1;
        let current = self.selected.unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, last) as usize;
        self.selected = Some(next);
        self.ensure_visible();
    }

    /// Scrolls so the selected row is within the visible window.
    fn ensure_visible(&mut self) {
        let rows = self.rows().max(1);
        if let Some(sel) = self.selected {
            if sel < self.top {
                self.top = sel;
            } else if sel >= self.top + rows {
                self.top = sel + 1 - rows;
            }
        }
    }

    /// Scrolls the view by `delta` rows (negative = up) **without** moving the
    /// selection — the wheel and scroll bar pan the list. Clamped so the last
    /// screenful never scrolls off the bottom.
    fn scroll_by(&mut self, delta: isize) {
        let max_top = self.items.len().saturating_sub(self.rows()) as isize;
        self.top = ((self.top as isize) + delta).clamp(0, max_top) as usize;
    }

    /// Handles a mouse event (in the list's local coordinates): a click selects the
    /// row under the pointer, or — on the scroll bar — scrolls; the wheel pans the
    /// view a row at a time.
    fn handle_mouse(&mut self, m: &MouseEvent) -> EventResult {
        let rows = self.rows();
        let width = self.bounds.width();
        let has_bar = self.items.len() > rows && width > 1;
        match m.kind {
            MouseKind::Down(MouseButton::Left) => {
                if has_bar && m.pos.x == width - 1 {
                    let mut bar = ScrollBar::new(
                        Rect::from_origin_size(Point::new(width - 1, 0), Size::new(1, rows as i16)),
                        self.style,
                    );
                    bar.set_metrics(self.items.len(), rows, self.top);
                    if let Some(part) = bar.hit(m.pos) {
                        let page = rows.max(1) as isize;
                        match part {
                            ScrollPart::LineUp => self.scroll_by(-1),
                            ScrollPart::LineDown => self.scroll_by(1),
                            ScrollPart::PageUp => self.scroll_by(-page),
                            ScrollPart::PageDown => self.scroll_by(page),
                            ScrollPart::Thumb => {}
                        }
                    }
                } else if m.pos.y >= 0 {
                    let idx = self.top + m.pos.y as usize;
                    if idx < self.items.len() {
                        self.selected = Some(idx);
                    }
                }
                EventResult::Consumed
            }
            MouseKind::ScrollDown => {
                self.scroll_by(1);
                EventResult::Consumed
            }
            MouseKind::ScrollUp => {
                self.scroll_by(-1);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

impl View for ListBox {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn draw(&self, canvas: &mut Canvas) {
        let area = canvas.bounds();
        canvas.fill(area, &Cell::blank(self.style));
        let rows = self.rows();
        if rows == 0 || area.width() <= 0 {
            return;
        }

        let needs_bar = self.items.len() > rows && area.width() > 1;
        let text_w = if needs_bar {
            area.width() - 1
        } else {
            area.width()
        };

        {
            let mut text = canvas.child(Rect::from_origin_size(
                Point::new(0, 0),
                Size::new(text_w, rows as i16),
            ));
            for r in 0..rows {
                let idx = self.top + r;
                if idx >= self.items.len() {
                    break;
                }
                let row_style = if self.focused && self.selected == Some(idx) {
                    self.focus_style
                } else {
                    self.style
                };
                let row = Rect::from_origin_size(Point::new(0, r as i16), Size::new(text_w, 1));
                text.fill(row, &Cell::blank(row_style));
                text.put_str(Point::new(0, r as i16), &self.items[idx], row_style);
            }
        }

        if needs_bar {
            let mut bar = ScrollBar::new(
                Rect::from_origin_size(Point::new(0, 0), Size::new(1, rows as i16)),
                self.style,
            );
            bar.set_metrics(self.items.len(), rows, self.top);
            let mut sub = canvas.child(Rect::from_origin_size(
                Point::new(text_w, 0),
                Size::new(1, rows as i16),
            ));
            bar.draw(&mut sub);
        }
    }

    fn handle_event(&mut self, event: &Event, _ctx: &mut Context) -> EventResult {
        if let Event::Mouse(m) = event {
            return self.handle_mouse(m);
        }
        if let Event::Key(key) = event {
            if self.focused {
                let page = self.rows().max(1) as isize;
                match key.code {
                    KeyCode::Up => {
                        self.move_by(-1);
                        return EventResult::Consumed;
                    }
                    KeyCode::Down => {
                        self.move_by(1);
                        return EventResult::Consumed;
                    }
                    KeyCode::PageUp => {
                        self.move_by(-page);
                        return EventResult::Consumed;
                    }
                    KeyCode::PageDown => {
                        self.move_by(page);
                        return EventResult::Consumed;
                    }
                    KeyCode::Home => {
                        self.move_by(isize::MIN / 2);
                        return EventResult::Consumed;
                    }
                    KeyCode::End => {
                        self.move_by(isize::MAX / 2);
                        return EventResult::Consumed;
                    }
                    _ => {}
                }
            }
        }
        EventResult::Ignored
    }

    fn focusable(&self) -> bool {
        true
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::command::CommandSet;
    use crate::event::{KeyEvent, Modifiers};

    fn items(labels: &[&str]) -> Vec<String> {
        labels.iter().map(|s| s.to_string()).collect()
    }

    fn rect(w: i16, h: i16) -> Rect {
        Rect::from_origin_size(Point::new(0, 0), Size::new(w, h))
    }

    fn list(w: i16, h: i16, labels: &[&str]) -> ListBox {
        let mut lb = ListBox::new(rect(w, h), items(labels), &Theme::default());
        lb.set_focused(true);
        lb
    }

    fn press(lb: &mut ListBox, code: KeyCode) -> EventResult {
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        lb.handle_event(&Event::Key(KeyEvent::new(code, Modifiers::NONE)), &mut ctx)
    }

    fn mouse(lb: &mut ListBox, kind: MouseKind, x: i16, y: i16) -> EventResult {
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        lb.handle_event(
            &Event::Mouse(MouseEvent {
                kind,
                pos: Point::new(x, y),
                modifiers: Modifiers::NONE,
            }),
            &mut ctx,
        )
    }

    #[test]
    fn clicking_a_row_selects_that_item() {
        let mut lb = list(10, 4, &["a", "b", "c", "d"]);
        mouse(&mut lb, MouseKind::Down(MouseButton::Left), 2, 2);
        assert_eq!(lb.selected(), Some(2));
        assert_eq!(lb.selected_text(), Some("c"));
    }

    #[test]
    fn clicking_below_the_last_item_does_nothing() {
        let mut lb = list(10, 6, &["a", "b"]);
        mouse(&mut lb, MouseKind::Down(MouseButton::Left), 0, 4); // blank row
        assert_eq!(lb.selected(), Some(0), "selection unchanged");
    }

    #[test]
    fn the_wheel_scrolls_the_list_without_moving_the_selection() {
        let mut lb = list(10, 3, &["a", "b", "c", "d", "e", "f"]);
        assert_eq!(lb.top, 0);
        mouse(&mut lb, MouseKind::ScrollDown, 5, 1);
        assert_eq!(lb.top, 1);
        assert_eq!(
            lb.selected(),
            Some(0),
            "the wheel does not move the selection"
        );
    }

    #[test]
    fn clicking_the_scroll_bar_down_arrow_scrolls() {
        // 6 items, 3 rows → a scroll bar is drawn at column 9 (width - 1).
        let mut lb = list(10, 3, &["a", "b", "c", "d", "e", "f"]);
        assert_eq!(lb.top, 0);
        mouse(&mut lb, MouseKind::Down(MouseButton::Left), 9, 2); // the ▼ at the bar's foot
        assert_eq!(lb.top, 1);
        assert_eq!(lb.selected(), Some(0), "scrolling leaves the selection");
    }

    fn render(lb: &ListBox, w: i16, h: i16) -> String {
        let mut buf = Buffer::new(Size::new(w, h));
        let mut canvas = Canvas::new(&mut buf);
        lb.draw(&mut canvas);
        buf.to_text()
    }

    #[test]
    fn first_item_is_selected_and_empty_has_none() {
        assert_eq!(list(10, 4, &["a", "b"]).selected(), Some(0));
        let empty = ListBox::new(rect(10, 4), vec![], &Theme::default());
        assert_eq!(empty.selected(), None);
        assert_eq!(empty.selected_text(), None);
    }

    #[test]
    fn arrows_move_the_selection_and_clamp() {
        let mut lb = list(10, 4, &["a", "b", "c"]);
        press(&mut lb, KeyCode::Down);
        assert_eq!(lb.selected(), Some(1));
        press(&mut lb, KeyCode::Down);
        press(&mut lb, KeyCode::Down); // clamps at the last
        assert_eq!(lb.selected(), Some(2));
        assert_eq!(lb.selected_text(), Some("c"));
        press(&mut lb, KeyCode::Up);
        assert_eq!(lb.selected(), Some(1));
    }

    #[test]
    fn home_end_and_page_jump() {
        let mut lb = list(10, 3, &["a", "b", "c", "d", "e", "f"]);
        press(&mut lb, KeyCode::End);
        assert_eq!(lb.selected(), Some(5));
        press(&mut lb, KeyCode::Home);
        assert_eq!(lb.selected(), Some(0));
        press(&mut lb, KeyCode::PageDown); // a page is 3 rows
        assert_eq!(lb.selected(), Some(3));
    }

    #[test]
    fn scrolls_to_keep_the_selection_visible() {
        // 6 items in a 3-row box: End shows the tail (d, e, f) with a scroll bar.
        let mut lb = list(6, 3, &["aa", "bb", "cc", "dd", "ee", "ff"]);
        press(&mut lb, KeyCode::End);
        let text = render(&lb, 6, 3);
        let rows: Vec<&str> = text.lines().collect();
        assert!(rows[0].starts_with("dd"));
        assert!(rows[1].starts_with("ee"));
        assert!(rows[2].starts_with("ff"));
        // The scroll bar occupies the last column (a down arrow on the last row).
        assert!(rows[2].ends_with('▼'));
    }

    #[test]
    fn enter_bubbles_for_the_default_button() {
        let mut lb = list(10, 4, &["a", "b"]);
        assert_eq!(press(&mut lb, KeyCode::Enter), EventResult::Ignored);
    }

    #[test]
    fn snapshot_list_with_scrollbar() {
        let lb = list(12, 3, &["alpha", "beta", "gamma", "delta", "epsilon"]);
        insta::assert_snapshot!(render(&lb, 12, 3));
    }
}
