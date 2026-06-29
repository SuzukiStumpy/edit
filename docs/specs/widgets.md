# Module spec: `rvision::widgets`

- **Status:** Done
- **Phase:** 4 (Application chrome) — kept current through Phases 5–6
- **Related ADRs:** 0003 (retained tree, commands up / broadcasts down), 0004 (three-phase dispatch), 0005 (colour roles), 0015 (owner-relative coords + `Canvas`), 0016 (application shell + menu overlay)

## Purpose

The Phase 4 chrome widget family: the concrete `View`s that make a screen look
like TurboVision — a desktop backdrop, framed windows, a status line, and a menu
bar with pull-downs. Reusable, editor-agnostic. These are the *furniture* around
the focus-and-content widgets.

**This file specs the chrome subset only.** The rest of `rvision::widgets` is
specced alongside the phase that built it:

- **Controls (Phase 5)** — `Label`, `Button`, `InputLine`, `CheckBox`,
  `RadioButtons`, `ListBox`/`ListViewer`, `ScrollBar`: see
  [`controls.md`](controls.md).
- **Dialogs (Phase 5)** — `Dialog`, `MessageBox`, `FileDialog` and the
  `Application::exec_view` modal loop: see [`dialog.md`](dialog.md).
- The editor view itself lives in the `edit` crate, not here
  ([`editor.md`](editor.md)); in Phase 6 it draws a horizontal+vertical
  `ScrollBar` along its window frame.

It is **not** the application root: the layout, draw-ordering, menu overlay, and
accelerator routing that tie these together live in `app::Shell` (ADR 0016,
[`shell.md`](shell.md)).

## Public interface

```rust
// --- Background: a backdrop fill ---
pub struct Background { bounds: Rect, cell: Cell }
impl Background {
    pub fn new(bounds: Rect, cell: Cell) -> Self;     // e.g. '░' in DesktopBackground
}

// --- Frame: a window border with title + close/zoom glyphs ---
pub struct Frame { title: String, active: bool, style: Style, title_style: Style }
impl Frame {
    pub fn new(title: &str, style: Style, title_style: Style) -> Self;
    pub fn active(self, active: bool) -> Self;        // builder; active = doubled corners
    pub fn set_active(&mut self, active: bool);
    // draws into the *whole* canvas it is given (the window's outer rect)
}

// --- Window: a framed box with an interior ---
pub struct Window { bounds: Rect, frame: Frame, active: bool, interior: Box<dyn View> }
impl Window {
    pub fn new(bounds: Rect, title: &str, theme: &Theme, interior: Box<dyn View>) -> Self;
    pub fn interior_bounds(&self) -> Rect;            // bounds inset by the frame
    pub fn is_active(&self) -> bool;
    pub fn set_active(&mut self, active: bool);        // the desktop marks the top window
}
impl View for Window { /* focusable; draws frame then interior; routes to interior */ }

// --- Desktop: backdrop + windows, with an active (top) window ---
// Owns concrete Windows (not Box<dyn View>) so it can mark the active one.
pub struct Desktop { bounds: Rect, backdrop: Cell, windows: Vec<Window>, .. }
impl Desktop {
    pub fn new(bounds: Rect, backdrop: Cell, windows: Vec<Window>) -> Self;
    pub fn active(&self) -> Option<usize>;
    pub fn set_bounds(&mut self, bounds: Rect);       // the shell relays out on resize
}
impl View for Desktop { /* fills backdrop, then z-order windows; focused → active */ }

// --- StatusLine: global hot-key items (carved to a region by the shell) ---
pub struct StatusItem { hint: String, label: String, key: KeyEvent, command: Command }
impl StatusItem {
    pub fn new(hint: &str, label: &str, key: KeyEvent, command: Command) -> Self;
}
pub struct StatusLine { bounds: Rect, items: Vec<StatusItem>, style: Style, key_style: Style }
impl StatusLine {
    pub fn new(bounds: Rect, items: Vec<StatusItem>, style: Style, key_style: Style) -> Self;
    pub fn set_bounds(&mut self, bounds: Rect);
}
impl View for StatusLine { /* a matching KeyEvent posts its (enabled) command */ }

// --- MenuBar + Menu: titles across the top, pull-downs below ---
pub struct MenuItem { label: String, command: Command, shortcut: Option<String> }
impl MenuItem {
    pub fn new(label: &str, command: Command) -> Self;
    pub fn with_shortcut(self, shortcut: &str) -> Self;   // display-only label
}
pub struct Menu { title: String, items: Vec<MenuItem> }   // title's first letter is its Alt hot-key
impl Menu { pub fn new(title: &str, items: Vec<MenuItem>) -> Self; }
pub struct MenuBar { bounds: Rect, menus: Vec<Menu>, open: Option<usize>, highlight: usize, .. }
impl MenuBar {
    pub fn new(bounds: Rect, menus: Vec<Menu>, theme: &Theme) -> Self;
    pub fn set_bounds(&mut self, bounds: Rect);
    pub fn is_open(&self) -> bool;
    pub fn close(&mut self);
    pub fn draw_overlay(&self, canvas: &mut Canvas);      // the pull-down, full-frame canvas
}
impl View for MenuBar { /* draws the bar; handle_event runs the menu state machine */ }
```

> The chrome constructors take their `bounds` because `app::Shell`/`edit::app`
> carve a region per widget from the live terminal size each frame and re-seat them
> via `set_bounds` on resize (ADR 0016). `Background` (a plain backdrop fill) is the
> exception — it is a leaf used where a static fill is wanted.

## Behaviour & invariants

- **Drawing.** Every chrome widget draws into the canvas it is handed, sized to
  its assigned region (the shell carves these from the live terminal size — ADR
  0016 — so widgets do not lay themselves out from their own `bounds`). All writes
  clip (ADR 0015).
- **Frame.** Single-line box; the title is centred-ish on the top border with a
  space either side; an *active* frame uses doubled-corner glyphs; close `[■]` and
  zoom `[↑]` glyphs sit on the top border (drawn only; drag/resize/click are Phase
  9). Degrades without panic for tiny rects.
- **Window.** Focusable. Interior is inset by one cell on every side. Draws the
  frame over its whole rect, then the interior through a `child()` sub-canvas; a
  key/command routes to the interior, which may post commands (bubble up).
- **Desktop.** Always fills its area with the backdrop first, then draws windows in
  vector order (index 0 bottom, last on top = active). Positional → topmost window
  under the pointer; focused (key/command) → the active window; broadcast → all.
- **StatusLine.** A `Key` whose code equals an item's `key` posts that item's
  command (enabled-gated by `Context`, ADR 0003) and is consumed; other events are
  ignored. Drawn left→right, each item's key glyph in `key_style`.
- **MenuBar / Menu.** A small state machine (ADR 0016), no modal loop yet:
  - *Closed:* consumes `Alt`+a title's first letter (opens that menu) and `F10`
    (opens the first menu). Every other event is ignored, so it never eats the
    editor's keys.
  - *Open:* modal — consumes every `Key`. `Left`/`Right` switch the open menu
    (wrap), `Up`/`Down` move the highlight (wrap), `Enter` posts the highlighted
    item's command and closes, `Esc` closes. A disabled item's command is gated by
    `Context` (never posted); selecting it closes the menu like TV.
  - The bar draws titles separated by spaces; the open title is highlighted.
    `draw_overlay` draws the pull-down box under the open title with items, their
    shortcuts right-aligned, the highlight in `MenuSelected`, disabled items in
    `MenuDisabled`. The overlay is the shell's last draw, over the whole frame.

## Collaborators

- `Canvas`/`Buffer` (draw), `geometry` (`Rect`/`Point`/`Size`), `cell::Cell`.
- `theme::{Role, Theme}` (colours by role, ADR 0005), `color::Style`.
- `view::{View, Group, Context}`, `command::{Command, CommandSet}`, `event` types.
- Widgets never reference one another: a control posts via `Context`; the shell
  routes events to them and draws them (ADR 0003, 0016).

## Test plan (write these first)

- **Render (snapshot):** backdrop fill; an active/inactive frame with title +
  glyphs; a window over a desktop; a status line; the menu bar closed and with one
  menu open (bar + pull-down overlay).
- **Interaction (scripted events):** status-line key posts the right command and a
  disabled one does not; menu opens on `Alt`-letter/`F10`, `Left`/`Right` and
  `Up`/`Down` move within wrap, `Enter` posts + closes, `Esc` closes; a closed menu
  bar ignores ordinary keys; a window routes keys to its interior.
- **Logic:** `Window::interior_bounds` insets by one; `Desktop::active` is the
  topmost window.
- **Manual:** the `chrome` example on a real terminal (see [`shell.md`](shell.md)).

## Open questions

- Underlined hot-key letters in titles/items (TV's `~X~`): use the first letter
  for now; richer markup is a later polish.
- Focus-aware frame styling reads a stored `active` flag set by the desktop; the
  general focus-in-draw question (ADR-tracked in `view.md`) is otherwise unchanged.
