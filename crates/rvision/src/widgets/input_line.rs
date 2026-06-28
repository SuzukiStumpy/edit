//! A one-line editable text field (TurboVision's `TInputLine`).
//!
//! A focusable [control](super) for a [`Dialog`](super::Dialog): it holds a
//! `String`, a grapheme cursor, and a horizontal scroll offset so a value longer
//! than the field stays usable. Editing and cursor motion step by **grapheme
//! cluster** (`unicode-segmentation`, ADR 0006/0008), never by byte. When focused
//! it draws a caret (a reverse-video cell, ADR 0017); a real hardware cursor
//! waits for the editor (Phase 6).

use crate::canvas::Canvas;
use crate::cell::{Cell, Grapheme};
use crate::color::{Attributes, Style};
use crate::event::{Event, EventResult, KeyCode, Modifiers};
use crate::geometry::{Point, Rect};
use crate::theme::{Role, Theme};
use crate::view::{Context, View};
use unicode_segmentation::UnicodeSegmentation;

/// A single-line text input field.
pub struct InputLine {
    bounds: Rect,
    text: String,
    /// Cursor position as a grapheme index in `0..=len`.
    cursor: usize,
    /// Leftmost visible grapheme index.
    scroll: usize,
    focused: bool,
    style: Style,
}

impl InputLine {
    /// Creates an empty input field at `bounds`, in the theme's [`Role::Input`]
    /// colour.
    pub fn new(bounds: Rect, theme: &Theme) -> Self {
        Self {
            bounds,
            text: String::new(),
            cursor: 0,
            scroll: 0,
            focused: false,
            style: theme.style(Role::Input),
        }
    }

    /// Seeds the field with `text`, placing the cursor at the end.
    pub fn with_text(mut self, text: &str) -> Self {
        self.text = text.to_string();
        self.cursor = self.len();
        self.scroll = 0;
        self.ensure_visible();
        self
    }

    /// The current value.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The cursor position (a grapheme index in `0..=len`).
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The number of grapheme clusters in the value.
    fn len(&self) -> usize {
        self.text.graphemes(true).count()
    }

    /// Byte offset of each grapheme start, with `text.len()` appended — so
    /// `starts[i]` is valid for every grapheme index `i` in `0..=len`.
    fn grapheme_starts(&self) -> Vec<usize> {
        let mut starts: Vec<usize> = self.text.grapheme_indices(true).map(|(i, _)| i).collect();
        starts.push(self.text.len());
        starts
    }

    /// The grapheme index whose start is at or after `byte` (clamped to `len`).
    fn byte_to_grapheme(&self, byte: usize) -> usize {
        self.grapheme_starts()
            .iter()
            .position(|&s| s >= byte)
            .unwrap_or_else(|| self.len())
    }

    /// Inserts `c` at the cursor, advancing past whatever grapheme now sits there
    /// (so a combining mark that merges with the previous cluster is handled).
    fn insert(&mut self, c: char) {
        let at = self.grapheme_starts()[self.cursor];
        self.text.insert(at, c);
        self.cursor = self.byte_to_grapheme(at + c.len_utf8());
        self.ensure_visible();
    }

    /// Removes the grapheme before the cursor.
    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let starts = self.grapheme_starts();
        self.text
            .replace_range(starts[self.cursor - 1]..starts[self.cursor], "");
        self.cursor -= 1;
        self.ensure_visible();
    }

    /// Removes the grapheme at the cursor.
    fn delete(&mut self) {
        if self.cursor >= self.len() {
            return;
        }
        let starts = self.grapheme_starts();
        self.text
            .replace_range(starts[self.cursor]..starts[self.cursor + 1], "");
        self.ensure_visible();
    }

    /// Scrolls so the cursor column is visible within the field width.
    fn ensure_visible(&mut self) {
        let width = self.bounds.width().max(1);
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
            return;
        }
        let graphemes: Vec<&str> = self.text.graphemes(true).collect();
        while col_of(&graphemes, self.cursor) - col_of(&graphemes, self.scroll) >= width {
            self.scroll += 1;
        }
    }
}

/// Display width of one grapheme cluster, in columns (≥ 0).
fn width_of(grapheme: &str) -> i16 {
    Grapheme::new(grapheme).width() as i16
}

/// Total display width of `graphemes[..idx]`.
fn col_of(graphemes: &[&str], idx: usize) -> i16 {
    graphemes[..idx.min(graphemes.len())]
        .iter()
        .map(|g| width_of(g))
        .sum()
}

impl View for InputLine {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn draw(&self, canvas: &mut Canvas) {
        let area = canvas.bounds();
        let width = area.width();
        canvas.fill(area, &Cell::blank(self.style));

        let graphemes: Vec<&str> = self.text.graphemes(true).collect();
        let mut col = 0;
        let mut idx = self.scroll;
        while idx < graphemes.len() && col < width {
            let w = width_of(graphemes[idx]).max(1);
            if col + w > width {
                break; // never split a wide grapheme across the right edge
            }
            canvas.put_str(Point::new(col, 0), graphemes[idx], self.style);
            col += w;
            idx += 1;
        }

        // The caret: a reverse-video cell over the grapheme at the cursor (or a
        // blank past the end), drawn only when focused (ADR 0017).
        if self.focused {
            let caret_col = col_of(&graphemes, self.cursor) - col_of(&graphemes, self.scroll);
            if caret_col >= 0 && caret_col < width {
                let style = self.style.attrs(Attributes::REVERSE);
                let cell = if self.cursor < graphemes.len() {
                    Cell::new(Grapheme::new(graphemes[self.cursor]), style)
                } else {
                    Cell::blank(style)
                };
                canvas.set(Point::new(caret_col, 0), cell);
            }
        }
    }

    fn handle_event(&mut self, event: &Event, _ctx: &mut Context) -> EventResult {
        let Event::Key(key) = event else {
            return EventResult::Ignored;
        };
        if !self.focused {
            return EventResult::Ignored;
        }
        match key.code {
            KeyCode::Char(c)
                if !c.is_control()
                    && !key.modifiers.contains(Modifiers::CONTROL)
                    && !key.modifiers.contains(Modifiers::ALT) =>
            {
                self.insert(c);
                EventResult::Consumed
            }
            KeyCode::Backspace => {
                self.backspace();
                EventResult::Consumed
            }
            KeyCode::Delete => {
                self.delete();
                EventResult::Consumed
            }
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                self.ensure_visible();
                EventResult::Consumed
            }
            KeyCode::Right => {
                if self.cursor < self.len() {
                    self.cursor += 1;
                }
                self.ensure_visible();
                EventResult::Consumed
            }
            KeyCode::Home => {
                self.cursor = 0;
                self.ensure_visible();
                EventResult::Consumed
            }
            KeyCode::End => {
                self.cursor = self.len();
                self.ensure_visible();
                EventResult::Consumed
            }
            // Tab / Enter / Esc and anything else bubble to the dialog.
            _ => EventResult::Ignored,
        }
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
    use crate::event::KeyEvent;
    use crate::geometry::Size;

    fn rect(w: i16) -> Rect {
        Rect::from_origin_size(Point::new(0, 0), Size::new(w, 1))
    }

    fn focused(width: i16) -> InputLine {
        let mut input = InputLine::new(rect(width), &Theme::default());
        input.set_focused(true);
        input
    }

    fn press(input: &mut InputLine, code: KeyCode) -> EventResult {
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        input.handle_event(&Event::Key(KeyEvent::new(code, Modifiers::NONE)), &mut ctx)
    }

    fn type_str(input: &mut InputLine, s: &str) {
        for c in s.chars() {
            press(input, KeyCode::Char(c));
        }
    }

    fn render(input: &InputLine, width: i16) -> String {
        let mut buf = Buffer::new(Size::new(width, 1));
        let mut canvas = Canvas::new(&mut buf);
        input.draw(&mut canvas);
        buf.to_text()
    }

    #[test]
    fn typing_inserts_and_advances_the_cursor() {
        let mut input = focused(20);
        type_str(&mut input, "abc");
        assert_eq!(input.text(), "abc");
        assert_eq!(input.cursor(), 3);
    }

    #[test]
    fn inserts_at_the_cursor_not_just_the_end() {
        let mut input = focused(20);
        type_str(&mut input, "ac");
        press(&mut input, KeyCode::Left); // between a and c
        press(&mut input, KeyCode::Char('b'));
        assert_eq!(input.text(), "abc");
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn backspace_and_delete_remove_around_the_cursor() {
        let mut input = focused(20);
        type_str(&mut input, "abc"); // cursor at 3
        press(&mut input, KeyCode::Backspace);
        assert_eq!(input.text(), "ab");
        press(&mut input, KeyCode::Home);
        press(&mut input, KeyCode::Delete);
        assert_eq!(input.text(), "b");
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn cursor_moves_by_grapheme_and_to_the_ends() {
        let mut input = focused(20);
        type_str(&mut input, "abc");
        press(&mut input, KeyCode::Home);
        assert_eq!(input.cursor(), 0);
        press(&mut input, KeyCode::Right);
        assert_eq!(input.cursor(), 1);
        press(&mut input, KeyCode::End);
        assert_eq!(input.cursor(), 3);
        press(&mut input, KeyCode::Left);
        assert_eq!(input.cursor(), 2);
        // Left at the start and Right at the end do not run off.
        press(&mut input, KeyCode::Home);
        press(&mut input, KeyCode::Left);
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn a_wide_grapheme_is_one_cursor_step() {
        let mut input = focused(20);
        type_str(&mut input, "a世b");
        assert_eq!(input.text(), "a世b");
        assert_eq!(input.cursor(), 3, "the wide char counts as one grapheme");
    }

    #[test]
    fn an_unfocused_field_ignores_typing() {
        let mut input = InputLine::new(rect(20), &Theme::default());
        assert_eq!(press(&mut input, KeyCode::Char('x')), EventResult::Ignored);
        assert_eq!(input.text(), "");
    }

    #[test]
    fn enter_and_tab_bubble_so_the_dialog_can_use_them() {
        let mut input = focused(20);
        assert_eq!(press(&mut input, KeyCode::Enter), EventResult::Ignored);
        assert_eq!(press(&mut input, KeyCode::Tab), EventResult::Ignored);
    }

    #[test]
    fn horizontal_scroll_keeps_the_cursor_visible() {
        // Field width 5; type 8 chars. The window scrolls to show the tail and
        // the caret sits in the last column.
        let mut input = focused(5);
        type_str(&mut input, "abcdefgh");
        assert_eq!(render(&input, 5), "efgh ", "shows the tail with the caret");
        // Home scrolls back to the start.
        press(&mut input, KeyCode::Home);
        assert_eq!(render(&input, 5), "abcde");
    }

    #[test]
    fn the_caret_is_reverse_video_only_when_focused() {
        let mut input = focused(10).with_text("hi");
        input.set_focused(true);
        let mut buf = Buffer::new(Size::new(10, 1));
        let mut canvas = Canvas::new(&mut buf);
        input.draw(&mut canvas);
        // Cursor is at the end (column 2): a reverse-video blank caret.
        assert!(
            buf.get(Point::new(2, 0))
                .unwrap()
                .style()
                .attrs
                .contains(Attributes::REVERSE)
        );

        // Unfocused: no caret anywhere.
        input.set_focused(false);
        let mut buf = Buffer::new(Size::new(10, 1));
        let mut canvas = Canvas::new(&mut buf);
        input.draw(&mut canvas);
        for x in 0..10 {
            assert!(
                !buf.get(Point::new(x, 0))
                    .unwrap()
                    .style()
                    .attrs
                    .contains(Attributes::REVERSE)
            );
        }
    }
}
