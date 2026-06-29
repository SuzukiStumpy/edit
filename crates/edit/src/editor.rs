//! The on-screen editor: an [`EditorView`] that owns one document and renders it
//! through a scrolling viewport, turning keystrokes into cursor motion and into
//! reversible [`Edit`]s (see `docs/specs/editor.md`).
//!
//! It is a plain [`View`] — no terminal, no files (load/save is Phase 6b). Editing
//! flows through the reversible [`Edit`] type (ADR 0011), journalled for undo by an
//! owned `History`: every mutation goes through `commit`, and
//! [`undo`](EditorView::undo)/[`redo`](EditorView::redo) replay inverses. The
//! clipboard lives in the app (ADR 0019): the editor only posts `CM_CUT`/`CM_COPY`/
//! `CM_PASTE` and exposes [`take_selection`](EditorView::take_selection) /
//! [`insert_text`](EditorView::insert_text) for the app to drive; undo/redo, by
//! contrast, are editor-local and handled here. Display geometry (tab expansion,
//! wide graphemes) lives in one place, `line_columns`, so rendering and vertical
//! cursor motion can never disagree.

use crate::history::{Coalesce, History};
use crate::search::{Match, Query};
use crate::text::{Edit, LineArray, Position, TextBuffer, position_after};
use rvision::canvas::Canvas;
use rvision::cell::{Cell, Grapheme};
use rvision::color::{Attributes, Style};
use rvision::command::{CM_USER, Command};
use rvision::event::{Event, EventResult, KeyCode, Modifiers, MouseButton, MouseEvent, MouseKind};
use rvision::geometry::{Point, Rect, Size};
use rvision::theme::{Role, Theme};
use rvision::view::{Context, View};
use unicode_segmentation::UnicodeSegmentation;

/// The default tab stop width in columns (ADR 0010 — display width 8).
const DEFAULT_TAB_WIDTH: usize = 8;

/// Lines the viewport pans per wheel notch (ADR 0007).
const WHEEL_STEP: i16 = 3;

/// Edit ▸ Cut — remove the selection to the clipboard. Posted by the editor,
/// acted on by the app, which owns the clipboard (ADR 0019).
pub const CM_CUT: Command = Command(CM_USER + 10);
/// Edit ▸ Copy — copy the selection to the clipboard.
pub const CM_COPY: Command = Command(CM_USER + 11);
/// Edit ▸ Paste — insert the clipboard at the caret.
pub const CM_PASTE: Command = Command(CM_USER + 12);
/// Edit ▸ Undo — reverse the last action (the editor handles this itself; the
/// menu posts it for the driver to route back here).
pub const CM_UNDO: Command = Command(CM_USER + 13);
/// Edit ▸ Redo — re-apply the last undone action.
pub const CM_REDO: Command = Command(CM_USER + 14);
/// Search ▸ Find — the editor posts this; the app runs the Find dialog and calls
/// [`find`](EditorView::find).
pub const CM_FIND: Command = Command(CM_USER + 15);
/// Search ▸ Find Next — repeat the last search (the editor handles the key; the
/// menu posts it for the driver to route to [`find_next`](EditorView::find_next)).
pub const CM_FIND_NEXT: Command = Command(CM_USER + 16);
/// Search ▸ Replace — the editor posts this; the app runs the Replace dialog and
/// calls [`replace_all`](EditorView::replace_all).
pub const CM_REPLACE: Command = Command(CM_USER + 17);
/// Search ▸ Go to Line — the editor posts this; the app runs the dialog and calls
/// [`go_to_line`](EditorView::go_to_line).
pub const CM_GOTO: Command = Command(CM_USER + 18);

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
    /// The undo/redo journal; also the source of truth for the dirty flag.
    history: History,
    /// The last search, for Find Next / Find Previous to repeat.
    last_query: Option<Query>,
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
            history: History::new(),
            last_query: None,
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
        self.history = History::new();
    }

    /// The whole document as a single `'\n'`-joined string.
    pub fn text(&self) -> String {
        self.doc.to_text()
    }

    /// The caret position.
    pub fn cursor(&self) -> Position {
        self.cursor
    }

    /// The number of lines in the document (always at least 1).
    pub fn line_count(&self) -> usize {
        self.doc.line_count()
    }

    /// Moves the caret to the start of 1-based `line`, clamped into the document,
    /// dropping the selection and revealing it. Used by the Go to Line dialog.
    pub fn go_to_line(&mut self, line: usize) {
        self.history.break_run();
        let last = self.doc.line_count();
        let target = line.clamp(1, last) - 1;
        self.cursor = Position::new(target, 0);
        self.anchor = None;
        self.goal_col = None;
        self.ensure_visible();
    }

    /// Runs `query` (remembering it for Find Next), selecting the first match in
    /// the given direction and revealing it. Returns whether a match was found.
    pub fn find(&mut self, query: Query, backward: bool) -> bool {
        self.last_query = Some(query);
        let query = self.last_query.clone().expect("just set");
        self.run_search(&query, backward)
    }

    /// Repeats the last search (Find Next / Find Previous), or returns `false` if
    /// there has been no search yet.
    pub fn find_next(&mut self, backward: bool) -> bool {
        let Some(query) = self.last_query.clone() else {
            return false;
        };
        self.run_search(&query, backward)
    }

    /// Whether a search has been run (so Find Next has something to repeat).
    pub fn has_query(&self) -> bool {
        self.last_query.is_some()
    }

    /// Replaces every match of `query` with `with` as a single undo unit; returns
    /// the number of replacements. Scans left-to-right without wrapping, continuing
    /// past each replacement so it never re-matches inside the inserted text.
    pub fn replace_all(&mut self, query: &Query, with: &str) -> usize {
        let before = self.cursor;
        let mut edits = Vec::new();
        let mut from = Position::default();
        let mut end = before;
        while let Some(found) = query.find(&self.doc, from, false, false) {
            let span = self.doc.slice(found.start, found.end);
            let delete = Edit::delete(found.start, span);
            let insert = Edit::insert(found.start, with);
            self.doc.apply(&delete);
            self.doc.apply(&insert);
            edits.push(delete);
            edits.push(insert);
            from = position_after(found.start, with);
            end = from;
        }
        if edits.is_empty() {
            return 0;
        }
        let count = edits.len() / 2;
        // The edits are already applied (we searched the live buffer), so record
        // them in the journal directly rather than through `commit`.
        self.cursor = end;
        self.anchor = None;
        self.goal_col = None;
        self.history
            .record(before, edits, end, Coalesce::Standalone);
        self.ensure_visible();
        count
    }

    /// Searches for `query` from just past the current match (forward) or before it
    /// (backward), wrapping, and selects the hit.
    fn run_search(&mut self, query: &Query, backward: bool) -> bool {
        let from = if backward {
            self.selection_range()
                .map_or(self.cursor, |(start, _)| start)
        } else {
            self.cursor
        };
        match query.find(&self.doc, from, backward, true) {
            Some(found) => {
                self.select_match(found);
                true
            }
            None => false,
        }
    }

    /// Selects `found` (anchor at its start, caret at its end) and reveals it.
    fn select_match(&mut self, found: Match) {
        self.history.break_run();
        self.anchor = Some(found.start);
        self.cursor = found.end;
        self.goal_col = None;
        self.ensure_visible();
    }

    /// Whether the document has unsaved changes (relative to the last save, even
    /// across undo/redo — ADR 0011).
    pub fn is_modified(&self) -> bool {
        self.history.is_modified()
    }

    /// Pins the current state as saved (call after a successful save); undoing or
    /// redoing back to it clears the dirty flag again.
    pub fn mark_saved(&mut self) {
        self.history.mark_saved();
    }

    /// Whether there is an action available to undo.
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Whether there is an action available to redo.
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Reverses the most recent action; returns whether anything was undone.
    pub fn undo(&mut self) -> bool {
        let Some(rec) = self.history.take_undo() else {
            return false;
        };
        for edit in rec.edits().iter().rev() {
            self.doc.apply(&edit.invert());
        }
        self.cursor = rec.before();
        self.anchor = None;
        self.goal_col = None;
        self.history.push_redo(rec);
        self.ensure_visible();
        true
    }

    /// Re-applies the most recently undone action; returns whether anything was
    /// redone.
    pub fn redo(&mut self) -> bool {
        let Some(rec) = self.history.take_redo() else {
            return false;
        };
        for edit in rec.edits() {
            self.doc.apply(edit);
        }
        self.cursor = rec.after();
        self.anchor = None;
        self.goal_col = None;
        self.history.push_undo(rec);
        self.ensure_visible();
        true
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

    /// The document [`Position`] under a **screen** point — the inverse of the draw
    /// mapping: screen → viewport-local (via `bounds`) → document (via the scroll
    /// offsets and tab/wide-grapheme expansion). A point above/left of the text
    /// clamps to the first line/column; below the last line, to the last line; past
    /// a line's end, to that line's end.
    fn position_at(&self, screen: Point) -> Position {
        let local_x = screen.x - self.bounds.origin().x;
        let local_y = screen.y - self.bounds.origin().y;
        let last_line = self.doc.line_count().saturating_sub(1);
        let line = (self.top + local_y.max(0) as usize).min(last_line);
        let target = self.left + local_x.max(0);
        let text = self.doc.line(line).unwrap_or("");
        Position::new(line, column_at_display(text, self.tab_width, target))
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

    /// Scrolls the viewport vertically by `delta` lines (negative = up) **without**
    /// moving the caret — the wheel pans the view, like every editor. Clamped so the
    /// first line never scrolls above the top.
    fn scroll_lines(&mut self, delta: i16) {
        let max_top = self.doc.line_count().saturating_sub(1) as i16;
        self.top = (self.top as i16 + delta).clamp(0, max_top) as usize;
    }

    // --- mouse ---

    /// Handles a mouse event whose position is in **screen** coordinates (the
    /// editor stores its `bounds` in screen space). A left-press drops the caret
    /// under the pointer and anchors a selection there; dragging extends it; the
    /// wheel pans the viewport. Editing/motion still flow through the keyboard
    /// paths, so this only moves the caret and view (ADR 0007).
    fn handle_mouse(&mut self, mouse: &MouseEvent) -> EventResult {
        match mouse.kind {
            MouseKind::Down(MouseButton::Left) => {
                self.history.break_run();
                let pos = self.position_at(mouse.pos);
                self.cursor = pos;
                // Anchor the drag origin; an empty span (anchor == caret) selects
                // nothing, so a plain click shows no selection.
                self.anchor = Some(pos);
                self.goal_col = None;
                self.ensure_visible();
                EventResult::Consumed
            }
            MouseKind::Drag(MouseButton::Left) => {
                // The anchor stays put, so the selection grows to the pointer.
                self.cursor = self.position_at(mouse.pos);
                self.goal_col = None;
                self.ensure_visible();
                EventResult::Consumed
            }
            MouseKind::ScrollDown => {
                self.scroll_lines(WHEEL_STEP);
                EventResult::Consumed
            }
            MouseKind::ScrollUp => {
                self.scroll_lines(-WHEEL_STEP);
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

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

    /// Common pre-amble for a motion key: a cursor move ends the current
    /// coalescing run (so a new typing burst is a fresh undo unit), then extend (or
    /// start) the selection when Shift is held, otherwise drop it.
    fn pre_move(&mut self, extend: bool) {
        self.history.break_run();
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

    // --- editing (all through reversible Edits journalled for undo) ---

    /// Applies `edits` in order as one undo unit, moves the caret to `after`, drops
    /// the selection, marks the goal column stale, and records the action in the
    /// history (where it may coalesce with the previous one — ADR 0011).
    fn commit(&mut self, before: Position, edits: Vec<Edit>, after: Position, coalesce: Coalesce) {
        for edit in &edits {
            self.doc.apply(edit);
        }
        self.cursor = after;
        self.anchor = None;
        self.goal_col = None;
        self.history.record(before, edits, after, coalesce);
    }

    /// The edit that removes the current selection and the position it collapses
    /// to, or `None` if nothing is selected. Pure — `commit` clears the anchor.
    fn selection_delete_edit(&self) -> Option<(Edit, Position)> {
        let (start, end) = self.selection_range()?;
        let span = self.doc.slice(start, end);
        Some((Edit::delete(start, span), start))
    }

    /// Deletes the current selection (if any) as one undo unit, leaving the caret
    /// at its start. Returns whether anything was deleted.
    fn delete_selection(&mut self) -> bool {
        let before = self.cursor;
        let Some((edit, start)) = self.selection_delete_edit() else {
            self.anchor = None;
            return false;
        };
        self.commit(before, vec![edit], start, Coalesce::Standalone);
        true
    }

    /// Inserts `text` at the caret as one undo unit (replacing any selection
    /// first), leaving the caret at the far end of the inserted text. Used for
    /// Paste; `text` may span lines.
    pub fn insert_text(&mut self, text: &str) {
        let before = self.cursor;
        let (mut edits, at) = match self.selection_delete_edit() {
            Some((edit, start)) => (vec![edit], start),
            None => (Vec::new(), self.cursor),
        };
        edits.push(Edit::insert(at, text));
        self.commit(
            before,
            edits,
            position_after(at, text),
            Coalesce::Standalone,
        );
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

    /// Inserts a single character at the caret (replacing any selection first). A
    /// run of plain typing coalesces into one undo unit; replacing a selection is
    /// its own unit.
    fn insert_char(&mut self, c: char) {
        let before = self.cursor;
        let (mut edits, at, coalesce) = match self.selection_delete_edit() {
            Some((edit, start)) => (vec![edit], start, Coalesce::Standalone),
            None => (Vec::new(), self.cursor, Coalesce::Typing),
        };
        edits.push(Edit::insert(at, c.to_string()));
        let after = Position::new(at.line, at.column + 1);
        self.commit(before, edits, after, coalesce);
        self.ensure_visible();
    }

    /// Splits the line at the caret (replacing any selection first). Enter is
    /// always its own undo unit.
    fn insert_newline(&mut self) {
        let before = self.cursor;
        let (mut edits, at) = match self.selection_delete_edit() {
            Some((edit, start)) => (vec![edit], start),
            None => (Vec::new(), self.cursor),
        };
        edits.push(Edit::insert(at, "\n"));
        let after = Position::new(at.line + 1, 0);
        self.commit(before, edits, after, Coalesce::Standalone);
        self.ensure_visible();
    }

    /// Backspace: delete the selection, else the grapheme before the caret,
    /// joining the previous line at column 0. In-line deletes coalesce into a run.
    fn backspace(&mut self) {
        if self.delete_selection() {
            self.ensure_visible();
            return;
        }
        let before = self.cursor;
        if self.cursor.column > 0 {
            let from = Position::new(self.cursor.line, self.cursor.column - 1);
            let span = self.doc.slice(from, self.cursor);
            self.commit(
                before,
                vec![Edit::delete(from, span)],
                from,
                Coalesce::Deleting,
            );
        } else if self.cursor.line > 0 {
            let prev = self.cursor.line - 1;
            let join = Position::new(prev, self.line_len(prev));
            self.commit(
                before,
                vec![Edit::delete(join, "\n")],
                join,
                Coalesce::Standalone,
            );
        }
        self.ensure_visible();
    }

    /// Delete: remove the selection, else the grapheme at the caret, joining the
    /// next line at end-of-line. In-line deletes coalesce into a run.
    fn delete_forward(&mut self) {
        if self.delete_selection() {
            self.ensure_visible();
            return;
        }
        let at = self.cursor;
        if self.cursor.column < self.line_len(self.cursor.line) {
            let to = Position::new(self.cursor.line, self.cursor.column + 1);
            let span = self.doc.slice(self.cursor, to);
            self.commit(at, vec![Edit::delete(at, span)], at, Coalesce::Deleting);
        } else if self.cursor.line + 1 < self.doc.line_count() {
            self.commit(at, vec![Edit::delete(at, "\n")], at, Coalesce::Standalone);
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
        let key = match event {
            Event::Key(key) => key,
            Event::Mouse(mouse) => return self.handle_mouse(mouse),
            _ => return EventResult::Ignored,
        };
        let shift = key.modifiers.contains(Modifiers::SHIFT);
        let ctrl = key.modifiers.contains(Modifiers::CONTROL);
        let alt = key.modifiers.contains(Modifiers::ALT);
        // These keys post a command for the app to act on: the clipboard ones
        // because the app owns the clipboard (ADR 0019), Go to Line because the app
        // runs the dialog. The editor touches nothing here.
        match key.code {
            KeyCode::Char('c' | 'C') if ctrl && !alt => return self.post(ctx, CM_COPY),
            KeyCode::Char('x' | 'X') if ctrl && !alt => return self.post(ctx, CM_CUT),
            KeyCode::Char('v' | 'V') if ctrl && !alt => return self.post(ctx, CM_PASTE),
            KeyCode::Char('f' | 'F') if ctrl && !alt => return self.post(ctx, CM_FIND),
            KeyCode::Char('g' | 'G') if ctrl && !alt => return self.post(ctx, CM_GOTO),
            KeyCode::Insert if ctrl => return self.post(ctx, CM_COPY),
            KeyCode::Insert if shift => return self.post(ctx, CM_PASTE),
            KeyCode::Delete if shift => return self.post(ctx, CM_CUT),
            _ => {}
        }
        match key.code {
            // Undo/redo are editor-local (the journal lives here), so unlike the
            // clipboard the editor acts on them directly rather than posting.
            KeyCode::Char('z' | 'Z') if ctrl && !alt && shift => {
                self.redo();
                EventResult::Consumed
            }
            KeyCode::Char('z' | 'Z') if ctrl && !alt => {
                self.undo();
                EventResult::Consumed
            }
            KeyCode::Char('y' | 'Y') if ctrl && !alt => {
                self.redo();
                EventResult::Consumed
            }
            // Find Next/Previous repeat the last search — editor-local, like undo.
            KeyCode::F(3) if !ctrl && !alt => {
                self.find_next(shift);
                EventResult::Consumed
            }
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

    // --- go to line (7c) ---

    #[test]
    fn go_to_line_moves_to_the_line_start_and_clamps() {
        let mut e = editor(20, 4).clone_doc("one\ntwo\nthree\nfour");
        e.go_to_line(3); // 1-based
        assert_eq!(e.cursor(), Position::new(2, 0));
        e.go_to_line(99); // past the end clamps to the last line
        assert_eq!(e.cursor(), Position::new(3, 0));
        e.go_to_line(0); // below 1 clamps to the first line
        assert_eq!(e.cursor(), Position::new(0, 0));
    }

    #[test]
    fn ctrl_g_posts_the_go_to_line_command() {
        let mut e = editor(20, 4).clone_doc("a\nb");
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        let r = e.handle_event(
            &Event::Key(KeyEvent::new(KeyCode::Char('g'), Modifiers::CONTROL)),
            &mut ctx,
        );
        assert_eq!(r, EventResult::Consumed);
        assert_eq!(ctx.take_posted(), vec![Event::Command(CM_GOTO)]);
    }

    // --- find (7c.2) ---

    #[test]
    fn find_selects_the_match_and_find_next_walks_both_ways() {
        use crate::search::Query;
        let mut e = editor(40, 4).clone_doc("foo bar foo baz foo");
        assert!(e.find(Query::new("foo"), false));
        assert_eq!(e.selected_text().as_deref(), Some("foo"));
        assert_eq!(e.cursor(), Position::new(0, 3), "first match 0..3");
        assert!(e.find_next(false));
        assert_eq!(e.cursor(), Position::new(0, 11), "second match 8..11");
        assert!(e.find_next(true));
        assert_eq!(e.cursor(), Position::new(0, 3), "back to the first match");
    }

    #[test]
    fn find_reports_absence_and_find_next_needs_a_prior_query() {
        use crate::search::Query;
        let mut e = editor(20, 4).clone_doc("hello");
        assert!(!e.find_next(false), "nothing to repeat yet");
        assert!(!e.find(Query::new("zzz"), false), "no such text");
        assert!(e.has_query(), "but the query is remembered");
    }

    #[test]
    fn ctrl_f_posts_find_and_f3_repeats_locally() {
        use crate::search::Query;
        let mut e = editor(20, 4).clone_doc("ab ab");
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        e.handle_event(
            &Event::Key(KeyEvent::new(KeyCode::Char('f'), Modifiers::CONTROL)),
            &mut ctx,
        );
        assert_eq!(ctx.take_posted(), vec![Event::Command(CM_FIND)]);
        // With a stored query, F3 advances without the app's help.
        e.find(Query::new("ab"), false); // selects 0..2
        key(&mut e, KeyCode::F(3));
        assert_eq!(e.cursor(), Position::new(0, 5), "F3 found the next 'ab'");
    }

    // --- replace (7c.3) ---

    #[test]
    fn replace_all_changes_every_match_as_one_undo_unit() {
        use crate::search::Query;
        let mut e = editor(40, 4).clone_doc("foo bar foo baz foo");
        assert_eq!(e.replace_all(&Query::new("foo"), "X"), 3);
        assert_eq!(e.text(), "X bar X baz X");
        assert!(e.is_modified());
        e.undo();
        assert_eq!(
            e.text(),
            "foo bar foo baz foo",
            "a single undo restores all"
        );
    }

    #[test]
    fn replace_all_copes_with_growth_and_deletion() {
        use crate::search::Query;
        let mut grow = editor(40, 4).clone_doc("aaa");
        assert_eq!(grow.replace_all(&Query::new("a"), "bb"), 3);
        assert_eq!(grow.text(), "bbbbbb", "no re-match inside the replacement");
        let mut shrink = editor(40, 4).clone_doc("a-a-a");
        assert_eq!(shrink.replace_all(&Query::new("a"), ""), 3);
        assert_eq!(shrink.text(), "--");
    }

    #[test]
    fn replace_all_spans_lines_and_reports_zero_when_absent() {
        use crate::search::Query;
        let mut e = editor(40, 5).clone_doc("cat\ndog\ncat");
        assert_eq!(e.replace_all(&Query::new("cat"), "x"), 2);
        assert_eq!(e.text(), "x\ndog\nx");
        assert_eq!(e.replace_all(&Query::new("zzz"), "q"), 0);
        assert_eq!(
            e.text(),
            "x\ndog\nx",
            "no match leaves the document untouched"
        );
    }

    // --- undo / redo (the journal; ADR 0011, 7b) ---

    #[test]
    fn undo_then_redo_round_trips_a_typing_run() {
        let mut e = editor(20, 5);
        type_str(&mut e, "hello"); // one coalesced undo unit
        assert!(e.can_undo() && !e.can_redo());
        assert!(e.undo());
        assert_eq!(e.text(), "", "the whole run undoes at once");
        assert_eq!(e.cursor(), Position::new(0, 0));
        assert!(e.redo());
        assert_eq!(e.text(), "hello");
        assert_eq!(e.cursor(), Position::new(0, 5));
        assert!(!e.redo(), "nothing left to redo");
    }

    #[test]
    fn a_cursor_move_splits_typing_into_separate_undo_units() {
        let mut e = editor(20, 5);
        type_str(&mut e, "ab");
        key(&mut e, KeyCode::Left); // breaks the run
        type_str(&mut e, "c"); // inserted at column 1
        assert_eq!(e.text(), "acb");
        e.undo();
        assert_eq!(e.text(), "ab", "only the post-move keystroke undoes first");
        e.undo();
        assert_eq!(e.text(), "");
    }

    #[test]
    fn a_backspace_run_undoes_as_one_unit_restoring_the_caret() {
        let mut e = editor(20, 5).clone_doc("abcd");
        key(&mut e, KeyCode::End); // caret at (0, 4)
        key(&mut e, KeyCode::Backspace);
        key(&mut e, KeyCode::Backspace); // "ab"
        assert_eq!(e.text(), "ab");
        assert!(e.undo());
        assert_eq!(e.text(), "abcd", "both deletes restore together");
        assert_eq!(
            e.cursor(),
            Position::new(0, 4),
            "caret back where the run began"
        );
    }

    #[test]
    fn a_multiline_paste_is_one_undo_unit() {
        let mut e = editor(20, 5);
        e.insert_text("one\ntwo");
        assert_eq!(e.text(), "one\ntwo");
        assert!(e.undo());
        assert_eq!(e.text(), "");
    }

    #[test]
    fn typing_over_a_selection_undoes_to_the_original_text() {
        let mut e = editor(20, 5).clone_doc("hello world");
        for _ in 0..5 {
            press(&mut e, KeyCode::Right, Modifiers::SHIFT); // select "hello"
        }
        type_str(&mut e, "X"); // replace-selection: one Standalone unit
        assert_eq!(e.text(), "X world");
        e.undo();
        assert_eq!(e.text(), "hello world");
    }

    #[test]
    fn a_new_edit_after_undo_discards_the_redo_branch() {
        let mut e = editor(20, 5);
        type_str(&mut e, "ab");
        e.undo();
        assert!(e.can_redo());
        type_str(&mut e, "z");
        assert!(!e.can_redo(), "diverging from the undone state drops redo");
        assert_eq!(e.text(), "z");
    }

    #[test]
    fn undoing_past_the_save_point_re_marks_modified() {
        let mut e = editor(20, 5);
        type_str(&mut e, "x");
        assert!(e.is_modified());
        e.mark_saved();
        assert!(!e.is_modified());
        e.undo();
        assert!(e.is_modified(), "undone past the save => dirty");
        e.redo();
        assert!(!e.is_modified(), "redone back to the save => clean");
    }

    #[test]
    fn ctrl_z_and_ctrl_y_drive_undo_and_redo() {
        let mut e = editor(20, 5);
        type_str(&mut e, "hi");
        press(&mut e, KeyCode::Char('z'), Modifiers::CONTROL);
        assert_eq!(e.text(), "");
        press(&mut e, KeyCode::Char('y'), Modifiers::CONTROL);
        assert_eq!(e.text(), "hi");
        // Ctrl+Shift+Z also redoes (after an undo).
        press(&mut e, KeyCode::Char('z'), Modifiers::CONTROL);
        press(
            &mut e,
            KeyCode::Char('z'),
            Modifiers::CONTROL | Modifiers::SHIFT,
        );
        assert_eq!(e.text(), "hi");
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

    // --- mouse (Phase 9c) ---
    //
    // The test editor's bounds sit at the origin, so screen and viewport-local
    // coordinates coincide here.

    fn mouse(e: &mut EditorView, kind: MouseKind, x: i16, y: i16) -> EventResult {
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        e.handle_event(
            &Event::Mouse(MouseEvent {
                kind,
                pos: Point::new(x, y),
                modifiers: Modifiers::NONE,
            }),
            &mut ctx,
        )
    }

    fn left_down(e: &mut EditorView, x: i16, y: i16) -> EventResult {
        mouse(e, MouseKind::Down(MouseButton::Left), x, y)
    }

    #[test]
    fn clicking_places_the_caret_under_the_pointer() {
        let mut e = editor(20, 6).clone_doc("hello\nworld");
        left_down(&mut e, 2, 0);
        assert_eq!(e.cursor(), Position::new(0, 2));
        left_down(&mut e, 3, 1);
        assert_eq!(e.cursor(), Position::new(1, 3));
    }

    #[test]
    fn clicking_past_a_line_end_clamps_to_the_end() {
        let mut e = editor(20, 6).clone_doc("hello");
        left_down(&mut e, 99, 0);
        assert_eq!(e.cursor(), Position::new(0, 5)); // just past the last grapheme
    }

    #[test]
    fn clicking_below_the_text_clamps_to_the_last_line() {
        let mut e = editor(20, 6).clone_doc("a\nb");
        left_down(&mut e, 0, 9);
        assert_eq!(e.cursor().line, 1);
    }

    #[test]
    fn a_plain_click_selects_nothing() {
        let mut e = editor(20, 6).clone_doc("hello");
        left_down(&mut e, 2, 0);
        assert_eq!(e.selected_text(), None);
    }

    #[test]
    fn dragging_extends_a_selection_from_the_press() {
        let mut e = editor(20, 6).clone_doc("hello");
        left_down(&mut e, 1, 0); // anchor at column 1
        mouse(&mut e, MouseKind::Drag(MouseButton::Left), 4, 0); // caret to column 4
        assert_eq!(e.selected_text().as_deref(), Some("ell"));
        assert_eq!(e.cursor(), Position::new(0, 4));
    }

    #[test]
    fn the_wheel_pans_the_view_without_moving_the_caret() {
        let mut e = editor(20, 4).clone_doc("l0\nl1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9");
        assert_eq!(e.scroll_metrics().top, 0);
        mouse(&mut e, MouseKind::ScrollDown, 0, 0);
        assert_eq!(e.scroll_metrics().top, WHEEL_STEP as usize);
        assert_eq!(e.cursor(), Position::new(0, 0), "the caret stays put");
        mouse(&mut e, MouseKind::ScrollUp, 0, 0);
        assert_eq!(e.scroll_metrics().top, 0);
    }
}
