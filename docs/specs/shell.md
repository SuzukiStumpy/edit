# Module spec: `rvision::app::Shell`

- **Status:** Done
- **Phase:** 4 (Application chrome)
- **Related ADRs:** 0016 (this composite), 0003/0004 (tree + dispatch it sits on), 0015 (`Canvas`), 0005 (roles)

## Purpose

The standard application screen — TurboVision's `TProgram`: a menu bar across the
top, a status line across the bottom, and a [`Desktop`](widgets.md) between. It is
the one purpose-built root the generic [`Group`](view.md) cannot be (ADR 0016),
because it lays out live, draws the menu overlay last, and routes keys in three
local passes. It is an ordinary [`View`], so it lives inside [`Root`] and runs in
the [`Application`] loop unchanged.

It is **not** the loop (`Application`/`Program`) nor a widget — it is the thing
that arranges the Phase 4 widgets into a screen and wires their events.

## Public interface

```rust
pub struct Shell { menu_bar: MenuBar, desktop: Desktop, status_line: StatusLine, size: Size }
impl Shell {
    pub fn new(size: Size, menu_bar: MenuBar, desktop: Desktop, status_line: StatusLine) -> Self;
    pub fn menu_is_open(&self) -> bool;
}
impl View for Shell { /* draws all four, routes events (below) */ }
```

## Behaviour & invariants

- **Live layout.** `regions(size)` carves three rectangles: menu bar `= row 0`,
  status line `= row h-1`, desktop `= rows 1..h-1` (all full width; heights clamp
  at small sizes). `draw` uses `canvas.size()`; `Event::Resize` calls `relayout`,
  which stores the size and `set_bounds` on each child. The stored size feeds
  positional routing between frames.
- **Draw order (ADR 0016).** desktop → status line → menu bar, then
  `MenuBar::draw_overlay` over the **whole** frame, so an open pull-down sits on
  top of the desktop just below the bar. The menu bar's own region is one row, so
  its pull-down can only be drawn as this final overlay.
- **Key routing — three local passes (TurboVision pre/focused/post):**
  1. *menu bar* (pre-process): claims `Alt`+a title letter and `F10`; while a menu
     is open it is **modal**, consuming every key so nothing leaks to the editor.
  2. *desktop* (focused): the active window, whose interior may post commands.
  3. *status line* (post-process): global function-key / `Alt`-X hot-keys, tried
     only after the focused view declined — so they never shadow typing.
  The first pass to consume wins (`EventResult::or_else` chaining, ADR 0004).
- **Other events.** `Mouse` → the region under the pointer in its local
  coordinates (behaviour mostly Phase 9; the seam exists now). `Command`
  (re-dispatched by `Root`) → the desktop's active window; `CM_QUIT` never reaches
  the shell, as `Root` claims it before re-dispatch. `Broadcast`/`Idle` → all
  three children. `Resize` → `relayout` + tell the desktop.

## Collaborators

- [`widgets::{MenuBar, Desktop, StatusLine}`](widgets.md) (owned by value),
  [`Canvas`] (draw), `geometry` (`Rect`/`Size`/`Point`), `event`/`command`.
- Drops into [`Root`] (the loop bridge) as its root `View`; commands a child posts
  bubble to `Root`, which re-dispatches or claims `CM_QUIT` (ADR 0003).

## Test plan (write these first)

- **Render (snapshot):** a full 40×10 screen — menu titles, blue backdrop, a
  framed active window, status line.
- **Interaction:** a plain key reaches the active window; `Alt`-letter opens a menu
  and then swallows typing (the window never sees it); a function key falls through
  to the status line; a resize relays the chrome out (status line on the new bottom
  row).
- **End-to-end:** through `Application`, `Alt`-X reaches the status line, which
  posts `CM_QUIT`, and the loop exits.
- **Manual:** `cargo run -p rvision --example chrome` — see [`widgets.md`](widgets.md).

## Open questions

- **Mouse behaviour** (click-to-open menus, click-outside-to-close, drag) — Phase 9.
- **`exec_view` reconciliation.** The Phase 5 modal loop now exists
  ([`dialog.md`](dialog.md)), so the pull-down *could* become a modal view; left as
  a later cleanup — the shell's layout/overlay and the `MenuBar` data are unchanged.
