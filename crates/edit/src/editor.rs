//! The on-screen editor: an [`EditorView`] that owns one document and renders it
//! through a scrolling viewport, turning keystrokes into cursor motion and into
//! reversible [`Edit`]s (see `docs/specs/editor.md`).
//!
//! It is a plain [`View`] — no terminal, no files (load/save is Phase 6b), no undo
//! stack (Phase 7b). Editing already flows through the reversible [`Edit`] type
//! (ADR 0011); the journal that makes undo possible comes later. The clipboard
//! itself lives in the app (ADR 0019): the editor only posts `CM_CUT`/`CM_COPY`/
//! `CM_PASTE` and exposes [`take_selection`](EditorView::take_selection) /
//! [`insert_text`](EditorView::insert_text) for the app to drive. Display geometry
//! (tab expansion, wide graphemes) lives in one place, [`line_columns`], so
//! rendering and vertical cursor motion can never disagree.

use crate::text::{Edit, LineArray, Position, TextBuffer, position_after};
use rvision::canvas::Canvas;
use rvision::cell::{Cell, Grapheme};
use rvision::color::{Attributes, Style};
use rvision::command::{CM_USER, Command};
use rvision::event::{Event, EventResult, KeyCode, Modifiers};
use rvision::geometry::{Point, Rect, Size};
use rvision::theme::{Role, Theme};
use rvision::view::{Context, View};
use unicode_segmentation::UnicodeSegmentation;

/// The default tab stop width in columns (ADR 0010 — display width 8).
const DEFAULT_TAB_WIDTH: usize = 8;

/// Edit ▸ Cut — remove the selection to the clipboard. Posted by the editor,
/// acted on by the app, which owns the clipboard (ADR 0019).
pub const CM_CUT: Command = Command(CM_USER + 10);
/// Edit ▸ Copy — copy the selection to the clipboard.
pub const CM_COPY: Command = Command(CM_USER + 11);
/// Edit ▸ Paste — insert the clipboard at the caret.
pub const CM_PASTE: Command = Command(CM_USER + 12);

/// A scrolling text editor over one owned [`LineArray`] document.
pub struct EditorView {
    bounds: Rect,
    doc: LineArray,
    /// The caret, as a `(line, grapheme-column)` position, always valid.
    cursor: Position,
    /// The other end of the selection, or `None` when nothing is selected.
    anchor: Option<Position>,
    /// First visible line (vertical scroll).
    top: usize,
    /// First visible **display** column (horizontal scroll).
    left: i16,
    /// Sticky display column for vertical motion; cleared by horizontal motion/edit.
    goal_col: Option<i16>,
    tab_width: usize,
    focused: bool,
    /// Whether the document has unsaved changes.
    modified: bool,
    text_style: Style,
    selection_style: Style,
}

impl EditorView {
    /// Creates an editor showing an empty document at `bounds`, in the theme's
    /// [`Role::EditorText`]/[`Role::Selection`] colours.
    pub fn new(bounds: Rect, theme: &Theme) -> Self {
        Self {
            bounds,
            doc: LineArray::new(),
            cursor: Position::default(),
            anchor: None,
            top: 0,
            left: 0,
            goal_col: None,
            tab_width: DEFAULT_TAB_WIDTH,
            focused: false,
            modified: false,
            text_style: theme.style(Role::EditorText),
            selection_style: theme.style(Role::Selection),
        }
    }

    /// Builder: seed the document with `text`.
    pub fn with_text(mut self, text: &str) -> Self {
        self.set_text(text);
        self
    }

    /// Replaces the document with `text`, resetting the cursor, scroll, selection
    /// and dirty flag (as on opening a freshly loaded file).
    pub fn set_text(&mut self, text: &str) {
        self.doc = LineArray::from(text);
        self.cursor = Position::default();
        self.anchor = None;
        self.top = 0;
        self.left = 0;
        self.goal_col = None;
        self.modified = false;
    }

    /// The whole document as a single `'\n'`-joined string.
    pub fn text(&self) -> String {
        self.doc.to_text()
    }

    /// The caret position.
    pub fn cursor(&self) -> Position {
        self.cursor
    }

    /// Whether the document has unsaved changes.
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Clears the dirty flag (call after a successful save).
    pub fn mark_saved(&mut self) {
        self.modified = false;
    }

    /// The selected text, or `None` if the selection is empty (sets up the Phase 7
    /// clipboard).
    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection_range()?;
        Some(self.doc.slice(start, end))
    }

    /// Repositions the viewport (e.g. when the host window is resized), re-clamping
    /// the scroll so the cursor stays visible.
    pub fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
        self.ensure_visible();
    }

    /// Scroll metrics for drawing the host window's scroll bars: the document's
    /// extent (lines and widest display column), the visible viewport, and the
    /// current scroll offsets (top line, left display column). All in the editor's
    /// display units.
    pub fn scroll_metrics(&self) -> ScrollMetrics {
        ScrollMetrics {
            lines: self.doc.line_count(),
            content_width: (0..self.doc.line_count())
                .filter_map(|i| self.doc.line(i))
                .map(|line| display_width(line, self.tab_width))
                .max()
                .unwrap_or(0),
            viewport: self.bounds.size(),
            top: self.top,
            left: self.left,
        }
    }

    // --- geometry ---

    /// The display column at which the caret currently sits.
    fn cursor_display_col(&self) -> i16 {
        self.display_col(self.cursor)
    }

    /// The display column of `pos` (its grapheme column expanded for tabs/wide
    /// graphemes).
    fn display_col(&self, pos: Position) -> i16 {
        let line = self.doc.line(pos.line).unwrap_or("");
        let cols = line_columns(line, self.tab_width);
        cols[pos.column.min(cols.len() - 1)]
    }

    /// The number of grapheme columns on `line`.
    fn line_len(&self, line: usize) -> usize {
        self.doc.line_graphemes(line)
    }

    /// The grapheme cluster at `pos`, or `None` past the line's end.
    fn grapheme_at(&self, pos: Position) -> Option<String> {
        let line = self.doc.line(pos.line)?;
        line.graphemes(true).nth(pos.column).map(str::to_string)
    }

    /// Whether the grapheme at `pos` is part of a word (alphanumeric / underscore);
    /// a position at or past a line end counts as a non-word boundary.
    fn is_word_at(&self, pos: Position) -> bool {
        self.grapheme_at(pos)
            .and_then(|g| g.chars().next())
            .is_some_and(is_word_char)
    }

    /// Posts `command` for the app to act on and reports the key consumed.
    fn post(&self, ctx: &mut Context, command: Command) -> EventResult {
        ctx.post(command);
        EventResult::Consumed
    }

    /// The ordered selection span `(start, end)`, or `None` if there is no
    /// selection or it is empty.
    fn selection_range(&self) -> Option<(Position, Position)> {
        use std::cmp::Ordering;
        let anchor = self.anchor?;
        match anchor.cmp(&self.cursor) {
            Ordering::Equal => None,
            Ordering::Less => Some((anchor, self.cursor)),
            Ordering::Greater => Some((self.cursor, anchor)),
        }
    }

    // --- scrolling ---

    /// Scrolls the minimum needed to keep the caret inside the viewport.
    fn ensure_visible(&mut self) {
        let w = self.bounds.width().max(1);
        let h = self.bounds.height().max(1) as usize;
        if self.cursor.line < self.top {
            self.top = self.cursor.line;
        } else if self.cursor.line >= self.top + h {
            self.top = self.cursor.line - h + 1;
        }
        let cx = self.cursor_display_col();
        if cx < self.left {
            self.left = cx;
        } else if cx >= self.left + w {
            self.left = cx - w + 1;
        }
    }

    // --- cursor motion ---

    /// Common pre-amble for a motion key: extend (or start) the selection when
    /// Shift is held, otherwise drop it.
    fn pre_move(&mut self, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
    }

    /// Moves the caret horizontally by one grapheme, crossing lines at the ends.
    fn move_horizontal(&mut self, forward: bool, extend: bool) {
        self.pre_move(extend);
        let next = if forward {
            self.doc.grapheme_after(self.cursor)
        } else {
            self.doc.grapheme_before(self.cursor)
        };
        if let Some(pos) = next {
            self.cursor = pos;
        }
        self.goal_col = None;
        self.ensure_visible();
    }

    /// Moves the caret up or down `rows` lines, keeping the goal display column.
    fn move_vertical(&mut self, down: bool, rows: usize, extend: bool) {
        self.pre_move(extend);
        let goal = match self.goal_col {
            Some(g) => g,
            None => {
                let g = self.display_col(self.cursor);
                self.goal_col = Some(g);
                g
            }
        };
        let last = self.doc.line_count() - 1;
        let line = if down {
            (self.cursor.line + rows).min(last)
        } else {
            self.cursor.line.saturating_sub(rows)
        };
        let target_line = self.doc.line(line).unwrap_or("");
        self.cursor = Position::new(line, column_at_display(target_line, self.tab_width, goal));
        self.ensure_visible();
    }

    /// Moves to the start (`end == false`) or end of the current line.
    fn move_line_edge(&mut self, end: bool, extend: bool) {
        self.pre_move(extend);
        self.cursor.column = if end {
            self.line_len(self.cursor.line)
        } else {
            0
        };
        self.goal_col = None;
        self.ensure_visible();
    }

    /// Moves to the very start or end of the document.
    fn move_doc_edge(&mut self, end: bool, extend: bool) {
        self.pre_move(extend);
        self.cursor = if end {
            let last = self.doc.line_count() - 1;
            Position::new(last, self.line_len(last))
        } else {
            Position::default()
        };
        self.goal_col = None;
        self.ensure_visible();
    }

    /// Moves to the start of the next word (skipping any non-word run, then the
    /// word) or, backward, to the start of the previous word.
    fn move_word(&mut self, forward: bool, extend: bool) {
        self.pre_move(extend);
        if forward {
            // Skip the rest of the current word, then the non-word run, landing on
            // the next word's first grapheme (or the document end).
            while self.is_word_at(self.cursor) && self.step_right() {}
            while !self.is_word_at(self.cursor) && self.step_right() {}
        } else {
            // Step back over a non-word run, then over the word it precedes.
            while self.step_left_if(|w| !w) {}
            while self.step_left_if(|w| w) {}
        }
        self.goal_col = None;
        self.ensure_visible();
    }

    /// Moves the caret one grapheme right; returns whether it moved.
    fn step_right(&mut self) -> bool {
        match self.doc.grapheme_after(self.cursor) {
            Some(pos) => {
                self.cursor = pos;
                true
            }
            None => false,
        }
    }

    /// Moves the caret left by one grapheme iff the grapheme to its left satisfies
    /// `want` (applied to "is that grapheme a word char"); returns whether it moved.
    fn step_left_if(&mut self, want: impl Fn(bool) -> bool) -> bool {
        match self.doc.grapheme_before(self.cursor) {
            Some(prev) if want(self.is_word_at(prev)) => {
                self.cursor = prev;
                true
            }
            _ => false,
        }
    }

    // --- editing (all through reversible Edits) ---

    /// Applies `edit`, marks the document dirty, and clears the goal column.
    fn apply(&mut self, edit: &Edit) {
        self.doc.apply(edit);
        self.modified = true;
        self.goal_col = None;
    }

    /// Deletes the current selection (if any) as one edit, leaving the caret at its
    /// start. Returns whether anything was deleted.
    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection_range() else {
            self.anchor = None;
            return false;
        };
        let span = self.doc.slice(start, end);
        self.apply(&Edit::delete(start, span));
        self.cursor = start;
        self.anchor = None;
        true
    }

    /// Inserts `text` at the caret as one [`Edit`] (replacing any selection
    /// first), leaving the caret at the far end of the inserted text. Used for
    /// Paste; `text` may span lines.
    pub fn insert_text(&mut self, text: &str) {
        self.delete_selection();
        let at = self.cursor;
        self.apply(&Edit::insert(at, text));
        self.cursor = position_after(at, text);
        self.ensure_visible();
    }

    /// Removes the current selection and returns its text, or `None` if nothing is
    /// selected. Used for Cut — the app stores the returned text on the clipboard
    /// (ADR 0019).
    pub fn take_selection(&mut self) -> Option<String> {
        let text = self.selected_text()?;
        self.delete_selection();
        self.ensure_visible();
        Some(text)
    }

    /// Inserts a single character at the caret (replacing any selection first).
    fn insert_char(&mut self, c: char) {
        self.delete_selection();
        let at = self.cursor;
        self.apply(&Edit::insert(at, c.to_string()));
        self.cursor.column += 1;
        self.ensure_visible();
    }

    /// Splits the line at the caret (replacing any selection first).
    fn insert_newline(&mut self) {
        self.delete_selection();
        let at = self.cursor;
        self.apply(&Edit::insert(at, "\n"));
        self.cursor = Position::new(at.line + 1, 0);
        self.ensure_visible();
    }

    /// Backspace: delete the selection, else the grapheme before the caret,
    /// joining the previous line at column 0.
    fn backspace(&mut self) {
        if self.delete_selection() {
            self.ensure_visible();
            return;
        }
        if self.cursor.column > 0 {
            let from = Position::new(self.cursor.line, self.cursor.column - 1);
            let span = self.doc.slice(from, self.cursor);
            self.apply(&Edit::delete(from, span));
            self.cursor = from;
        } else if self.cursor.line > 0 {
            let prev = self.cursor.line - 1;
            let join = Position::new(prev, self.line_len(prev));
            self.apply(&Edit::delete(join, "\n"));
            self.cursor = join;
        }
        self.ensure_visible();
    }

    /// Delete: remove the selection, else the grapheme at the caret, joining the
    /// next line at end-of-line.
    fn delete_forward(&mut self) {
        if self.delete_selection() {
            self.ensure_visible();
            return;
        }
        if self.cursor.column < self.line_len(self.cursor.line) {
            let to = Position::new(self.cursor.line, self.cursor.column + 1);
            let span = self.doc.slice(self.cursor, to);
            self.apply(&Edit::delete(self.cursor, span));
        } else if self.cursor.line + 1 < self.doc.line_count() {
            self.apply(&Edit::delete(self.cursor, "\n"));
        }
        self.ensure_visible();
    }

    // --- drawing ---

    /// Draws one document line into `row`, expanding tabs and applying horizontal
    /// scroll; `selection` is the selected grapheme range on this line, if any.
    fn draw_line(
        &self,
        canvas: &mut Canvas,
        row: i16,
        line: &str,
        width: i16,
        selection: Option<LineSelection>,
    ) {
        let cols = line_columns(line, self.tab_width);
        for (i, grapheme) in line.graphemes(true).enumerate() {
            let start = cols[i] - self.left;
            let end = cols[i + 1] - self.left;
            if end <= 0 {
                continue; // fully scrolled off the left
            }
            if start >= width {
                break; // past the right edge
            }
            let selected = selection.is_some_and(|s| i >= s.from && i < s.to);
            let style = self.style_for(selected);
            if grapheme == "\t" {
                for x in start.max(0)..end.min(width) {
                    canvas.set(Point::new(x, row), Cell::blank(style));
                }
            } else {
                canvas.put_str(Point::new(start, row), grapheme, style);
            }
        }
        // A selection that runs past this line's end highlights the trailing blank.
        if selection.is_some_and(|s| s.through_eol) {
            let from = (cols[cols.len() - 1] - self.left).max(0);
            for x in from..width {
                canvas.set(Point::new(x, row), Cell::blank(self.selection_style));
            }
        }
    }

    /// The style for a normal (`false`) or selected (`true`) cell.
    fn style_for(&self, selected: bool) -> Style {
        if selected {
            self.selection_style
        } else {
            self.text_style
        }
    }

    /// The selected grapheme range on document line `line`, if the selection
    /// touches it.
    fn line_selection(&self, line: usize) -> Option<LineSelection> {
        let (start, end) = self.selection_range()?;
        if line < start.line || line > end.line {
            return None;
        }
        let from = if line == start.line { start.column } else { 0 };
        let to = if line == end.line {
            end.column
        } else {
            self.line_len(line)
        };
        Some(LineSelection {
            from,
            to,
            through_eol: line < end.line,
        })
    }
}

/// The selected grapheme range on a single rendered line.
#[derive(Clone, Copy)]
struct LineSelection {
    /// First selected grapheme index.
    from: usize,
    /// One past the last selected grapheme index.
    to: usize,
    /// Whether the selection continues past this line's end (highlight the rest).
    through_eol: bool,
}

/// Whether `c` counts as part of a word for word-wise motion.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Viewport/scroll metrics for an [`EditorView`], in display units — enough to
/// drive a vertical and a horizontal scroll bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollMetrics {
    /// Total document lines.
    pub lines: usize,
    /// Display width of the widest line.
    pub content_width: i16,
    /// The visible window size.
    pub viewport: Size,
    /// First visible line (vertical scroll offset).
    pub top: usize,
    /// First visible display column (horizontal scroll offset).
    pub left: i16,
}

/// The total display width of `line`: the column just past its last grapheme,
/// with tabs expanded to `tab_width` stops and wide graphemes counted as two.
fn display_width(line: &str, tab_width: usize) -> i16 {
    let tab = tab_width.max(1) as i16;
    let mut col: i16 = 0;
    for grapheme in line.graphemes(true) {
        col = if grapheme == "\t" {
            (col / tab + 1) * tab
        } else {
            col + Grapheme::new(grapheme).width().max(1) as i16
        };
    }
    col
}

/// The display column at which each grapheme of `line` begins, with the line's
/// total display width appended — so the result has `graphemes + 1` entries and
/// `cols[i]` is valid for every grapheme column `i` in `0..=len`. Tabs advance to
/// the next multiple of `tab_width`; wide graphemes occupy two columns.
fn line_columns(line: &str, tab_width: usize) -> Vec<i16> {
    let tab = tab_width.max(1) as i16;
    let mut cols = Vec::new();
    let mut col: i16 = 0;
    for grapheme in line.graphemes(true) {
        cols.push(col);
        col = if grapheme == "\t" {
            (col / tab + 1) * tab
        } else {
            col + Grapheme::new(grapheme).width().max(1) as i16
        };
    }
    cols.push(col);
    cols
}

/// The grapheme column on `line` whose display column is the last at or before
/// `goal` — the inverse of [`line_columns`], used to keep a vertical-motion goal.
fn column_at_display(line: &str, tab_width: usize, goal: i16) -> usize {
    let cols = line_columns(line, tab_width);
    let mut idx = 0;
    for (i, &c) in cols.iter().enumerate() {
        if c <= goal {
            idx = i;
        } else {
            break;
        }
    }
    idx
}

impl View for EditorView {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn draw(&self, canvas: &mut Canvas) {
        let Size { width, height } = canvas.size();
        canvas.fill(canvas.bounds(), &Cell::blank(self.text_style));
        for row in 0..height {
            let line_index = self.top + row as usize;
            let Some(line) = self.doc.line(line_index) else {
                break;
            };
            let selection = self.line_selection(line_index);
            self.draw_line(canvas, row, line, width, selection);
        }
        // The caret: a reverse-video cell over the grapheme at the cursor (drawn
        // last, only when focused — ADR 0017). No hardware cursor yet.
        if self.focused {
            let cy = self.cursor.line as i16 - self.top as i16;
            let cx = self.cursor_display_col() - self.left;
            if cy >= 0 && cy < height && cx >= 0 && cx < width {
                let style = self.text_style.attrs(Attributes::REVERSE);
                let cell = match self.grapheme_at(self.cursor) {
                    Some(ref g) if g != "\t" => Cell::new(Grapheme::new(g), style),
                    _ => Cell::blank(style),
                };
                canvas.set(Point::new(cx, cy), cell);
            }
        }
    }

    fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        let Event::Key(key) = event else {
            return EventResult::Ignored;
        };
        let shift = key.modifiers.contains(Modifiers::SHIFT);
        let ctrl = key.modifiers.contains(Modifiers::CONTROL);
        let alt = key.modifiers.contains(Modifiers::ALT);
        // Clipboard keys post a command for the app to act on (it owns the
        // clipboard — ADR 0019); the editor itself touches nothing here.
        match key.code {
            KeyCode::Char('c' | 'C') if ctrl && !alt => return self.post(ctx, CM_COPY),
            KeyCode::Char('x' | 'X') if ctrl && !alt => return self.post(ctx, CM_CUT),
            KeyCode::Char('v' | 'V') if ctrl && !alt => return self.post(ctx, CM_PASTE),
            KeyCode::Insert if ctrl => return self.post(ctx, CM_COPY),
            KeyCode::Insert if shift => return self.post(ctx, CM_PASTE),
            KeyCode::Delete if shift => return self.post(ctx, CM_CUT),
            _ => {}
        }
        match key.code {
            KeyCode::Char(c) if !c.is_control() && !ctrl && !alt => {
                self.insert_char(c);
                EventResult::Consumed
            }
            KeyCode::Enter => {
                self.insert_newline();
                EventResult::Consumed
            }
            KeyCode::Tab if !ctrl && !alt => {
                self.insert_char('\t');
                EventResult::Consumed
            }
            KeyCode::Backspace => {
                self.backspace();
                EventResult::Consumed
            }
            KeyCode::Delete => {
                self.delete_forward();
                EventResult::Consumed
            }
            KeyCode::Left if ctrl => {
                self.move_word(false, shift);
                EventResult::Consumed
            }
            KeyCode::Right if ctrl => {
                self.move_word(true, shift);
                EventResult::Consumed
            }
            KeyCode::Left => {
                self.move_horizontal(false, shift);
                EventResult::Consumed
            }
            KeyCode::Right => {
                self.move_horizontal(true, shift);
                EventResult::Consumed
            }
            KeyCode::Up => {
                self.move_vertical(false, 1, shift);
                EventResult::Consumed
            }
            KeyCode::Down => {
                self.move_vertical(true, 1, shift);
                EventResult::Consumed
            }
            KeyCode::PageUp => {
                self.move_vertical(false, self.bounds.height().max(1) as usize, shift);
                EventResult::Consumed
            }
            KeyCode::PageDown => {
                self.move_vertical(true, self.bounds.height().max(1) as usize, shift);
                EventResult::Consumed
            }
            KeyCode::Home if ctrl => {
                self.move_doc_edge(false, shift);
                EventResult::Consumed
            }
            KeyCode::End if ctrl => {
                self.move_doc_edge(true, shift);
                EventResult::Consumed
            }
            KeyCode::Home => {
                self.move_line_edge(false, shift);
                EventResult::Consumed
            }
            KeyCode::End => {
                self.move_line_edge(true, shift);
                EventResult::Consumed
            }
            // Esc and anything else bubble up to the menu/app (6c).
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
    use rvision::buffer::Buffer;
    use rvision::command::CommandSet;
    use rvision::event::KeyEvent;

    fn rect(w: i16, h: i16) -> Rect {
        Rect::from_origin_size(Point::new(0, 0), Size::new(w, h))
    }

    fn editor(w: i16, h: i16) -> EditorView {
        let mut e = EditorView::new(rect(w, h), &Theme::default());
        e.set_focused(true);
        e
    }

    fn press(e: &mut EditorView, code: KeyCode, mods: Modifiers) -> EventResult {
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        e.handle_event(&Event::Key(KeyEvent::new(code, mods)), &mut ctx)
    }

    fn key(e: &mut EditorView, code: KeyCode) -> EventResult {
        press(e, code, Modifiers::NONE)
    }

    fn type_str(e: &mut EditorView, s: &str) {
        for c in s.chars() {
            match c {
                '\n' => key(e, KeyCode::Enter),
                '\t' => key(e, KeyCode::Tab),
                _ => key(e, KeyCode::Char(c)),
            };
        }
    }

    fn render(e: &EditorView, w: i16, h: i16) -> String {
        let mut buf = Buffer::new(Size::new(w, h));
        let mut canvas = Canvas::new(&mut buf);
        e.draw(&mut canvas);
        buf.to_text()
    }

    // --- geometry ---

    #[test]
    fn line_columns_expands_tabs_and_wide_graphemes() {
        // Tab stops every 8: "a\tb" -> a at 0, tab fills to 8, b at 8.
        assert_eq!(line_columns("a\tb", 8), vec![0, 1, 8, 9]);
        // A wide grapheme takes two columns.
        assert_eq!(line_columns("a世b", 8), vec![0, 1, 3, 4]);
    }

    #[test]
    fn column_at_display_inverts_line_columns() {
        // "a\tb": display cols [0,1,8,9]. A goal mid-tab lands on the tab grapheme.
        assert_eq!(column_at_display("a\tb", 8, 0), 0);
        assert_eq!(column_at_display("a\tb", 8, 4), 1); // inside the tab
        assert_eq!(column_at_display("a\tb", 8, 8), 2);
        assert_eq!(column_at_display("a\tb", 8, 99), 3); // clamps to end
    }

    // --- typing & editing ---

    #[test]
    fn typing_inserts_text_and_advances_the_cursor() {
        let mut e = editor(20, 5);
        type_str(&mut e, "hello");
        assert_eq!(e.text(), "hello");
        assert_eq!(e.cursor(), Position::new(0, 5));
        assert!(e.is_modified());
    }

    #[test]
    fn enter_splits_the_line_and_moves_to_the_next() {
        let mut e = editor(20, 5);
        type_str(&mut e, "ab");
        key(&mut e, KeyCode::Left);
        key(&mut e, KeyCode::Enter);
        assert_eq!(e.text(), "a\nb");
        assert_eq!(e.cursor(), Position::new(1, 0));
    }

    #[test]
    fn tab_inserts_a_literal_tab() {
        let mut e = editor(20, 5);
        type_str(&mut e, "a");
        key(&mut e, KeyCode::Tab);
        type_str(&mut e, "b");
        assert_eq!(e.text(), "a\tb");
        assert_eq!(e.cursor(), Position::new(0, 3));
    }

    #[test]
    fn backspace_within_and_across_lines() {
        let mut e = editor(20, 5);
        type_str(&mut e, "ab\ncd");
        key(&mut e, KeyCode::Backspace); // remove 'd'
        assert_eq!(e.text(), "ab\nc");
        key(&mut e, KeyCode::Backspace); // remove 'c'
        assert_eq!(e.text(), "ab\n");
        key(&mut e, KeyCode::Backspace); // join lines
        assert_eq!(e.text(), "ab");
        assert_eq!(e.cursor(), Position::new(0, 2));
    }

    #[test]
    fn delete_within_and_across_lines() {
        let mut e = editor(20, 5);
        type_str(&mut e, "ab\ncd");
        key(&mut e, KeyCode::Home);
        key(&mut e, KeyCode::Delete); // remove 'c'
        assert_eq!(e.text(), "ab\nd");
        key(&mut e, KeyCode::Delete); // remove 'd' -> empty last line
        assert_eq!(e.text(), "ab\n");
        // From the end of line 0, Delete joins the next line.
        key(&mut e, KeyCode::Up);
        key(&mut e, KeyCode::End);
        key(&mut e, KeyCode::Delete);
        assert_eq!(e.text(), "ab");
    }

    // --- cursor motion ---

    #[test]
    fn vertical_motion_keeps_a_goal_column_through_short_lines() {
        let mut e = editor(20, 6).clone_doc("long line\nx\nanother one");
        // Cursor to column 7 on line 0.
        for _ in 0..7 {
            key(&mut e, KeyCode::Right);
        }
        assert_eq!(e.cursor(), Position::new(0, 7));
        key(&mut e, KeyCode::Down); // line 1 "x" is short: clamp to its end (col 1)
        assert_eq!(e.cursor(), Position::new(1, 1));
        key(&mut e, KeyCode::Down); // goal 7 restored on the long line 2
        assert_eq!(e.cursor(), Position::new(2, 7));
    }

    #[test]
    fn home_end_and_document_ends() {
        let mut e = editor(20, 6).clone_doc("alpha\nbeta\ngamma");
        key(&mut e, KeyCode::End);
        assert_eq!(e.cursor(), Position::new(0, 5));
        key(&mut e, KeyCode::Home);
        assert_eq!(e.cursor(), Position::new(0, 0));
        press(&mut e, KeyCode::End, Modifiers::CONTROL);
        assert_eq!(e.cursor(), Position::new(2, 5));
        press(&mut e, KeyCode::Home, Modifiers::CONTROL);
        assert_eq!(e.cursor(), Position::new(0, 0));
    }

    #[test]
    fn ctrl_arrows_move_by_word() {
        let mut e = editor(40, 4).clone_doc("foo bar_baz  qux");
        press(&mut e, KeyCode::Right, Modifiers::CONTROL);
        assert_eq!(e.cursor(), Position::new(0, 4), "to start of 'bar_baz'");
        press(&mut e, KeyCode::Right, Modifiers::CONTROL);
        assert_eq!(e.cursor(), Position::new(0, 13), "to start of 'qux'");
        press(&mut e, KeyCode::Left, Modifiers::CONTROL);
        assert_eq!(
            e.cursor(),
            Position::new(0, 4),
            "back to start of 'bar_baz'"
        );
    }

    // --- selection ---

    #[test]
    fn shift_motion_selects_and_typing_replaces_it() {
        let mut e = editor(20, 5).clone_doc("hello world");
        // Select "hello".
        for _ in 0..5 {
            press(&mut e, KeyCode::Right, Modifiers::SHIFT);
        }
        assert_eq!(e.selected_text().as_deref(), Some("hello"));
        type_str(&mut e, "HI");
        assert_eq!(e.text(), "HI world");
        assert_eq!(e.selected_text(), None);
    }

    #[test]
    fn an_unshifted_move_clears_the_selection() {
        let mut e = editor(20, 5).clone_doc("abc");
        press(&mut e, KeyCode::Right, Modifiers::SHIFT);
        assert!(e.selected_text().is_some());
        key(&mut e, KeyCode::Right);
        assert_eq!(e.selected_text(), None);
    }

    // --- clipboard primitives (the app owns the clipboard; ADR 0019) ---

    #[test]
    fn insert_text_pastes_multiline_and_lands_the_cursor_at_its_end() {
        let mut e = editor(20, 5).clone_doc("ab");
        key(&mut e, KeyCode::End); // cursor at (0, 2)
        e.insert_text("X\nYZ");
        assert_eq!(e.text(), "abX\nYZ");
        assert_eq!(e.cursor(), Position::new(1, 2));
        assert!(e.is_modified());
    }

    #[test]
    fn insert_text_replaces_an_active_selection() {
        let mut e = editor(20, 5).clone_doc("hello world");
        for _ in 0..5 {
            press(&mut e, KeyCode::Right, Modifiers::SHIFT); // select "hello"
        }
        e.insert_text("HI");
        assert_eq!(e.text(), "HI world");
        assert_eq!(e.cursor(), Position::new(0, 2));
        assert_eq!(e.selected_text(), None);
    }

    #[test]
    fn take_selection_returns_and_removes_it_then_is_none() {
        let mut e = editor(20, 5).clone_doc("hello");
        for _ in 0..3 {
            press(&mut e, KeyCode::Right, Modifiers::SHIFT); // select "hel"
        }
        assert_eq!(e.take_selection().as_deref(), Some("hel"));
        assert_eq!(e.text(), "lo");
        assert_eq!(e.cursor(), Position::new(0, 0));
        // With nothing selected it takes nothing and leaves the document alone.
        assert_eq!(e.take_selection(), None);
        assert_eq!(e.text(), "lo");
    }

    #[test]
    fn clipboard_keys_post_commands_without_touching_the_document() {
        let mut e = editor(20, 5).clone_doc("abc");
        for _ in 0..2 {
            press(&mut e, KeyCode::Right, Modifiers::SHIFT); // select "ab"
        }
        let posted = |e: &mut EditorView, code, mods| {
            let cs = CommandSet::new();
            let mut ctx = Context::new(&cs);
            let r = e.handle_event(&Event::Key(KeyEvent::new(code, mods)), &mut ctx);
            (r, ctx.take_posted())
        };
        let (r, p) = posted(&mut e, KeyCode::Char('c'), Modifiers::CONTROL);
        assert_eq!(r, EventResult::Consumed);
        assert_eq!(p, vec![Event::Command(CM_COPY)]);
        let (_, p) = posted(&mut e, KeyCode::Char('x'), Modifiers::CONTROL);
        assert_eq!(p, vec![Event::Command(CM_CUT)]);
        let (_, p) = posted(&mut e, KeyCode::Char('v'), Modifiers::CONTROL);
        assert_eq!(p, vec![Event::Command(CM_PASTE)]);
        // The classic accelerators map to the same commands.
        let (_, p) = posted(&mut e, KeyCode::Insert, Modifiers::CONTROL);
        assert_eq!(p, vec![Event::Command(CM_COPY)]);
        let (_, p) = posted(&mut e, KeyCode::Insert, Modifiers::SHIFT);
        assert_eq!(p, vec![Event::Command(CM_PASTE)]);
        let (_, p) = posted(&mut e, KeyCode::Delete, Modifiers::SHIFT);
        assert_eq!(p, vec![Event::Command(CM_CUT)]);
        // The keys posted commands only — the editor mutated nothing itself.
        assert_eq!(e.text(), "abc");
    }

    // --- viewport / scroll ---

    #[test]
    fn viewport_scrolls_vertically_to_keep_the_cursor_visible() {
        let text = (0..10)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut e = editor(10, 3).clone_doc(&text);
        press(&mut e, KeyCode::End, Modifiers::CONTROL);
        // Three rows tall; jumping to the document end scrolls so lines 7-9 show.
        assert_eq!(render(&e, 10, 3), "line7     \nline8     \nline9     ");
    }

    #[test]
    fn snapshot_caret_and_multiline_text() {
        let mut e = editor(12, 4).clone_doc("first line\nsecond");
        key(&mut e, KeyCode::Down);
        key(&mut e, KeyCode::Right);
        insta::assert_snapshot!(render(&e, 12, 4));
    }

    #[test]
    fn snapshot_horizontal_scroll_shows_the_tail() {
        let mut e = editor(8, 1).clone_doc("abcdefghijkl");
        key(&mut e, KeyCode::End);
        // Width 8: the caret at column 12 forces the view to scroll right.
        insta::assert_snapshot!(render(&e, 8, 1));
    }

    #[test]
    fn selected_cells_carry_the_selection_style() {
        // A text snapshot can't show colour, so assert the cell styles directly.
        let theme = Theme::default();
        let sel = theme.style(Role::Selection);
        let text = theme.style(Role::EditorText);
        let mut e = editor(12, 3).clone_doc("alpha\nbeta");
        press(&mut e, KeyCode::Down, Modifiers::SHIFT);
        press(&mut e, KeyCode::Right, Modifiers::SHIFT);
        press(&mut e, KeyCode::Right, Modifiers::SHIFT); // selection (0,0)..(1,2)

        let mut buf = Buffer::new(Size::new(12, 3));
        let mut canvas = Canvas::new(&mut buf);
        e.draw(&mut canvas);
        let bg = |x: i16, y: i16| buf.get(Point::new(x, y)).unwrap().style().bg;

        // Line 0 is wholly selected, including the trailing blank past its end.
        assert_eq!(bg(0, 0), sel.bg, "selected grapheme");
        assert_eq!(bg(7, 0), sel.bg, "through-EOL highlight");
        // Line 1: 'b','e' selected; 't' onward is normal text.
        assert_eq!(bg(0, 1), sel.bg);
        assert_eq!(bg(1, 1), sel.bg);
        assert_eq!(bg(3, 1), text.bg, "past the selection end");
    }

    #[test]
    fn caret_is_reverse_video_only_when_focused() {
        let mut e = editor(10, 1).clone_doc("hi"); // caret at (0,0) over 'h'
        let reverse_at = |e: &EditorView| {
            let mut buf = Buffer::new(Size::new(10, 1));
            let mut canvas = Canvas::new(&mut buf);
            e.draw(&mut canvas);
            buf.get(Point::new(0, 0))
                .unwrap()
                .style()
                .attrs
                .contains(Attributes::REVERSE)
        };
        assert!(reverse_at(&e), "focused: a reverse-video caret");
        e.set_focused(false);
        assert!(!reverse_at(&e), "unfocused: no caret");
    }

    /// Test helper: build a fresh focused editor with `text` loaded.
    impl EditorView {
        fn clone_doc(mut self, text: &str) -> Self {
            self.set_text(text);
            self.set_focused(true);
            self
        }
    }
}
