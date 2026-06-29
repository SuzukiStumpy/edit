# Module spec: `edit::editor`

- **Status:** Done
- **Phase:** 6 (editor, single document) — sub-phase 6a
- **Related ADRs:** 0006 (full Unicode), 0008 (line-array text), 0011 (reversible
  edits), 0015 (canvas), 0017 (focus-in-draw push)

## Purpose

The on-screen editor: an [`EditorView`] that **owns** one document
([`crate::text::LineArray`]) and renders it through a [`Canvas`] with a scrolling
viewport, a cursor, and an optional selection; it turns keystrokes into cursor
motion and into reversible [`Edit`]s applied to the document.

It is *not* responsible for files (load/save is 6b), for the undo journal
(Phase 7 — edits are already reversible, the stack comes later), or for the
clipboard (Phase 7). It holds no terminal and no app: it is a plain `View`, so
it composes inside a `Window`/`Desktop`/`Shell` and is unit-tested headlessly.

## Public interface

```rust
pub struct EditorView { /* doc, cursor, selection, scroll, goal_col, tab_width, focused, styles, modified */ }

impl EditorView {
    pub fn new(bounds: Rect, theme: &Theme) -> Self;       // empty document
    pub fn with_text(self, text: &str) -> Self;            // builder
    pub fn set_text(&mut self, text: &str);                // replace, cursor to (0,0)
    pub fn text(&self) -> String;                          // whole document
    pub fn cursor(&self) -> Position;
    pub fn is_modified(&self) -> bool;
    pub fn mark_saved(&mut self);                          // clear the dirty flag
    pub fn selected_text(&self) -> Option<String>;         // for Phase 7 clipboard
    pub fn set_bounds(&mut self, bounds: Rect);            // relayout
}

impl View for EditorView { /* bounds, draw, handle_event, focusable=true, set_focused */ }
```

## Behaviour & invariants

- **Cursor** is a `(line, grapheme-col)` [`Position`], always valid: line in
  `0..line_count`, column in `0..=line_graphemes(line)`.
- **Display geometry is one source of truth.** A `line_columns(line)` helper maps
  each grapheme boundary to a *display column*: tabs advance to the next multiple
  of `tab_width` (default 8, ADR 0010); wide graphemes take two columns. Both
  rendering and vertical-motion column matching use it, so they never disagree.
- **Vertical motion keeps a goal column** (a display column): Up/Down try to land
  at the same display column; it survives passing through short lines and is
  cleared by any horizontal motion or edit.
- **Editing goes through `Edit`** (insert/delete) applied to the owned doc, never
  ad-hoc string surgery — so every mutation is already invertible (ADR 0011).
  Each edit sets `modified`, moves the cursor to the edit's far end, clears the
  selection, and re-scrolls to keep the cursor visible.
- **An active selection is replaced** by typing/Backspace/Delete (delete the span
  as one `Edit`, then insert). With no selection those keys act at the cursor.
- **Viewport** scrolls minimally to keep the cursor visible in both axes; a resize
  (`set_bounds`) re-clamps it. Drawing clips to the canvas, so an over-long line or
  a line below the document is simply not drawn.
- **Cursor/selection render** as styled cells (reverse-video caret like
  `InputLine`; `Role::Selection` over the selected span), caret only when focused
  (ADR 0017). No hardware cursor yet.
- Edge cases: empty document (one blank line); cursor at end-of-line / end-of-doc;
  Backspace at column 0 joins the previous line; Delete at line end joins the next;
  tab + wide-char column math; horizontal scroll past the left edge.

## Collaborators

- `crate::text` — `LineArray`/`TextBuffer` (owned doc), `Position`, `Edit`,
  `slice` (new: text between two positions, for selection replace/copy).
- `rvision`: `Canvas`/`Cell`/`Style` (draw), `Theme`/`Role` (`EditorText`,
  `Selection`), `Event`/`KeyCode` (input), `View`/`Context`. Commands the editor
  cannot satisfy alone (Open/Save/Quit) bubble up unhandled (6c wires them).

## Test plan (write these first)

- **Logic:** `line_columns` tab/wide expansion; goal-column up/down across short
  and long lines; cursor clamping; selection span text.
- **Render (snapshot):** a multi-line doc with the caret; horizontal + vertical
  scroll showing only the viewport; a tab-indented line; a highlighted selection.
- **Interaction (scripted events):** type/Enter/Tab/Backspace/Delete scenarios
  assert document text + cursor; arrows/Home/End/PageUp/Down/Ctrl-Home/End move as
  expected; viewport scrolls to keep the cursor visible; Shift+motion selects and
  typing replaces the selection.
- **Manual:** `edit` itself once 6c lands.

## Open questions

- Word-wise motion (Ctrl-Left/Right) word definition — start simple (runs of
  word chars vs the rest); refine if it feels wrong by hand.
- Selection *clipboard* and the undo *stack* are Phase 7; this module only sets up
  reversible edits and a `selected_text()` accessor.
