# Module spec: `rvision::widgets` controls (Phase 5)

- **Status:** In progress
- **Phase:** 5 (Dialogs & controls)
- **Related ADRs:** 0003 (commands up / broadcasts down), 0004 (three-phase dispatch), 0005 (colour roles), 0015 (owner-relative coords + `Canvas`), 0017 (modal dialogs + focus-aware drawing)

## Purpose

The focusable, content-bearing widgets that live **inside** a [`Dialog`](dialog.md):
`Label`, `Button`, `InputLine`, `CheckBox`, `RadioButtons`, `ListBox`/`ListViewer`,
and `ScrollBar`. Reusable, editor-agnostic. These are the counterpart to the
Phase 4 chrome furniture (`Window`, `MenuBar`, …); together with `Dialog` they let
an application ask the user a question.

It is **not** the modal machinery (that is [`app::exec_view`](dialog.md)) and not
the editor view (Phase 6).

## Focus-aware drawing (ADR 0017)

`View` gains one defaulted method:

```rust
fn set_focused(&mut self, focused: bool) { let _ = focused; }
```

The owning `Group` pushes it as focus moves (initial child on construction; old →
`false`, new → `true` on Tab); a `Group` forwards a `set_focused` it receives to
its focused child, so it composes through nesting. A control that cares stores the
flag and consults it in `draw`; everything else inherits the no-op.

## Public interface

```rust
// --- Label: descriptive text in a dialog ---
pub struct Label { bounds: Rect, text: String, style: Style }
impl Label { pub fn new(bounds: Rect, text: &str, theme: &Theme) -> Self; }   // Role::EditorText-ish
impl View for Label { /* not focusable; draws its text */ }

// --- Button: posts a command when activated ---
pub struct Button { bounds, label, command, focused, default, normal, focused_style }
impl Button {
    pub fn new(bounds: Rect, label: &str, command: Command, theme: &Theme) -> Self;
    pub fn default(self, yes: bool) -> Self;   // Enter activates it even when unfocused
    pub fn is_default(&self) -> bool;
    pub fn command(&self) -> Command;
}
impl View for Button { /* focusable; Enter/Space → post(command); draws focused/normal */ }

// --- InputLine: a one-line text field ---
pub struct InputLine { bounds, text: String, cursor: usize /*grapheme*/, scroll, focused, .. }
impl InputLine {
    pub fn new(bounds: Rect, theme: &Theme) -> Self;
    pub fn with_text(self, text: &str) -> Self;
    pub fn text(&self) -> &str;
}
impl View for InputLine { /* focusable; edits text; draws caret when focused */ }

// --- CheckBox / RadioButtons ---
pub struct CheckBox { bounds, label, checked, focused, .. }
impl CheckBox { pub fn new(bounds, label, theme) -> Self; pub fn is_checked(&self) -> bool; }
pub struct RadioButtons { bounds, labels: Vec<String>, selected: usize, focused, .. }
impl RadioButtons { pub fn new(bounds, labels, theme) -> Self; pub fn selected(&self) -> usize; }

// --- ScrollBar + ListBox ---
pub struct ScrollBar { bounds, max, pos, vertical, .. }      // a drawn indicator (mouse: Phase 9)
pub struct ListBox { bounds, items: Vec<String>, selected, top, focused, .. }
impl ListBox {
    pub fn new(bounds: Rect, items: Vec<String>, theme: &Theme) -> Self;
    pub fn selected(&self) -> Option<usize>;
    pub fn selected_text(&self) -> Option<&str>;
}
impl View for ListBox { /* focusable; arrows/PgUp/PgDn/Home/End move, scrolls to keep selection in view */ }
```

## Behaviour & invariants

- **Label.** Not focusable; draws its text in its style, clipped. (TurboVision's
  hot-key→link focus transfer needs view IDs — deferred.)
- **Button.** Focusable. `Enter` or `Space` posts `command` (enabled-gated by
  `Context`, ADR 0003) and consumes the key. A *default* button's command is what
  the dialog posts on `Enter` from elsewhere (the dialog asks each button
  `is_default`). Draws filled in `ButtonFocused` when focused, else `ButtonNormal`,
  with the label centred (`[ Label ]`-ish). A disabled command never fires.
- **InputLine.** Focusable. Holds a `String` and a grapheme cursor index. Printable
  `Char` inserts at the cursor; `Backspace`/`Delete` remove the grapheme
  before/at it; `Left`/`Right`/`Home`/`End` move by grapheme; horizontal scroll
  keeps the cursor visible in a field narrower than the text. Draws the caret (a
  reverse cell) only when focused. Editing goes through grapheme-aware ops
  (`unicode-segmentation`, ADR 0008/0006) — no byte indexing.
- **CheckBox.** Focusable; `Space`/`Enter` toggles `checked`. Draws `[X]`/`[ ]` +
  label. **RadioButtons.** Focusable; `Up`/`Down` move the selection; draws
  `(•)`/`( )` per option; exactly one selected.
- **ListBox.** Focusable; `Up`/`Down` move the selection by one, `PgUp`/`PgDn` by a
  page, `Home`/`End` to the ends; `top` scrolls so the selection is always visible;
  the selected row draws in `Selection`. An empty list has `selected() == None`.
  Optional `ScrollBar` drawn on the right edge reflects `top`/len.
- All controls clip to their canvas (ADR 0015) and degrade without panic for tiny
  bounds and empty content.

## Collaborators

- `Canvas`/`Buffer`, `geometry`, `cell::Cell`, `theme::{Role, Theme}`, `color::Style`.
- `view::{View, Group, Context}`, `command::{Command, CommandSet}`, `event` types.
- `unicode-segmentation` for grapheme navigation in `InputLine` (shared with the
  text model, ADR 0008).
- Controls never reference one another: they post via `Context`; the `Dialog`'s
  `Group` routes and lays them out.

## Test plan (write these first)

- **Logic:** button command/default accessors; input-line insert/delete/cursor
  moves and scroll; checkbox toggle; radio selection wrap; list selection +
  scroll-to-keep-visible; grapheme handling for a wide/combining char in the input.
- **Render (snapshot):** focused vs unfocused button; an input line with caret;
  checkbox checked/unchecked; radio group; a list with a selection scrolled.
- **Interaction (scripted events):** button posts on Enter/Space, disabled does
  not; tab moves the focus flag (in `Group`); list arrows scroll.
- **Manual:** the `dialogs` example (Phase 5) on a real terminal.

## Open questions

- Hot-key underline markup (`~X~`) and Label→control focus links: need view IDs;
  deferred (same family as `view.md`'s ID deferral).
- Per-dialog command disabling (greying an OK button until valid) — see
  [`dialog.md`](dialog.md) open questions.
