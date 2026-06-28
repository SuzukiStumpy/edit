# ADR 0016 — Application shell: a `TProgram`-style root with a drawn menu overlay

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

Phase 4 assembles the standard application screen — a menu bar across the top, a
status line across the bottom, and a desktop (blue backdrop + windows) filling the
space between (roadmap Phase 4). TurboVision models this as `TProgram`/
`TApplication`: a single root group whose three permanent subviews are
`menuBar`, `deskTop`, and `statusLine`, laid out against the live screen size.

Three forces make the generic `Group` (ADR 0003) a poor fit for this root:

1. **Live layout.** A `Group` stores each child's `bounds` statically; the three
   chrome regions must be recomputed from the terminal size every frame so a
   resize relays them out. (Windows *inside* the desktop keep static bounds — they
   do not move on resize — so the generic `Group` still serves there.)
2. **The menu overlay.** An open pull-down must paint *below* the one-row menu bar,
   on top of the desktop. A `Group` hands each child a `Canvas` clipped to its
   `bounds` (ADR 0015), so a one-row menu bar cannot draw its pull-down, and a
   later sibling cannot be overdrawn by an earlier one. The overlay must be drawn
   last, over the whole frame.
3. **Accelerator routing.** Menu hot-keys (`Alt`-letter, `F10`) and status-line
   keys (`F1`, `Alt`-X) are *global*: they fire regardless of which view holds
   focus. TurboVision routes these through `phPreProcess`/`phPostProcess` passes
   that bracket the focused phase. Our three-phase engine (ADR 0004) has no such
   pass.

The interactive pull-down that TurboVision runs via a local modal loop
(`execView`) is, strictly, the Phase 5 `exec_view` mechanism. Phase 4 still owes
keyboard-driven menus, so it needs menu modality *before* `exec_view` exists.

## Decision

Add a purpose-built application-root view, **`app::Shell`** (TurboVision's
`TProgram`), instead of forcing the generic `Group`. It owns the three chrome
pieces as concrete typed fields — `MenuBar`, `Desktop`, `StatusLine` — and is
itself an ordinary `View`, so it drops into the existing `app::Root` loop bridge.

- **Live layout.** Each frame `Shell::draw` carves the menu-bar row (top), the
  status-line row (bottom), and the desktop (the rows between) from `canvas.size()`
  and draws each child through a `child()` sub-canvas (ADR 0015). It remembers the
  size for positional routing, refreshing it on `Event::Resize`.
- **Menu as a drawn overlay.** `Shell` draws desktop → status line → menu bar, then
  calls `MenuBar::draw_overlay` with a **full-frame** canvas so an open pull-down
  paints over everything. The pull-down is retained-tree state on the `MenuBar`
  (which menu is open, which item is highlighted), not a separate modal view.
- **Local pre/focus/post routing.** `Shell` reproduces TurboVision's three key
  passes *itself*, for its three children only: a key goes to the menu bar first
  (pre-process: it claims `Alt`-hot-keys, `F10`, and — while a menu is open —
  every key, modally), then to the desktop's active window (focused), then to the
  status line (post-process: global function-key hot-keys). The generic event
  engine and `Group` are **not** touched; pre/post-process stays confined to the
  shell, where the only views that need it live.

## Consequences

- **Faithful and self-contained.** The root mirrors `TProgram` and keeps the
  generic `Group`/`View`/`Canvas` seams (ADR 0003, 0004, 0015) unchanged; nothing
  speculative is added to the core event engine.
- **Menus work in Phase 4, reconciled in Phase 5.** Menu modality is a small
  hand-rolled state machine now. When `exec_view` (Phase 5) lands, the pull-down
  can be re-expressed as a modal view; the `MenuBar`'s data (titles, items,
  commands) and the shell's layout/overlay stay as they are.
- **One vestige.** The chrome children's own `bounds()` are not used by the shell
  for layout (it computes regions from the live size); they return their assigned
  region for trait-completeness and any internal positional math.
- **Deferred (noted, not built):** click-to-open and click-outside-to-close for
  menus, dragging a pull-down, and falling positional events through the menu bar
  to the desktop where it does not paint — all Phase 9 (mouse, ADR 0007). Generic
  pre/post-process passes on arbitrary `Group`s: add only when a non-shell view
  needs them (YAGNI).

## Alternatives considered

- **Generic `Group` with a full-screen menu bar.** Give the menu bar screen-tall
  `bounds` so its pull-down fits its clip. Rejected: it makes the menu bar swallow
  every positional event meant for the desktop (a Phase 9 rewrite), gives no live
  relayout, and misrepresents the menu bar's real extent.
- **Wait for `exec_view` (Phase 5) before any interactive menu.** Ship only static
  chrome in Phase 4. Rejected against the roadmap, which scopes keyboard menu
  navigation/dispatch to Phase 4; the shell's local modal routing delivers it now
  without blocking on the modal loop.
- **Generalise pre/post-process into the core dispatch now.** Add the passes to
  `Group` for every group. Rejected as speculative (ADR 0004 keeps the engine
  minimal): only the shell needs it today.
