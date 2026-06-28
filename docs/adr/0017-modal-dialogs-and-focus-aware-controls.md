# ADR 0017 — Modal dialogs (`exec_view`) and focus-aware control drawing

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

Phase 5 adds dialogs and the controls inside them — buttons, input lines, check
boxes, radio buttons, list boxes (roadmap Phase 5). Two gaps in the existing
machinery (ADR 0003, 0004, 0015, 0016) block them.

**1. Modality.** A dialog runs *modally*: it appears on top of the current
screen, takes every event until the user accepts or cancels, and yields a single
result (which button was pressed). TurboVision does this with `TGroup::execView`
— a nested event loop local to one view, distinct from the application's main
loop. Our main loop ([`Application::run`], ADR 0002) owns the terminal and drives
one root [`Program`]; a view deep in the tree handling an event has only a
[`Context`] for posting commands — no terminal, no way to start a nested loop.
Phase 4 already flagged this: the menu pull-down is a hand-rolled state machine
"reconciled in Phase 5" when the modal loop lands (ADR 0016).

**2. Focus-aware drawing.** A control must look different when focused — a focused
button is highlighted, a focused input line shows a cursor. But [`View::draw`]
takes only `&self` and a [`Canvas`] (ADR 0015); it is handed no focus state. The
owning [`Group`] knows which child is focused but draws every child the same way.
This was deferred from Phase 3 ("focus-aware drawing (Phase 5)", `view.md`) and
Phase 4 (active-frame styling at draw time, `shell.md`).

The tension on (1) is *where* the nested loop lives without re-entrancy through
the tree. The tension on (2) is *how* focus reaches `draw` without rewriting the
`draw` signature across every existing view.

## Decision

### Modal loop: `Application::exec_view`

Put the nested loop where the terminal already lives — on `Application`, beside
`run`:

```rust
pub fn exec_view(
    &mut self,
    background: &mut dyn Program,   // the screen behind, redrawn each frame
    dialog: &mut Dialog,
) -> io::Result<Command>;
```

Each turn it builds a frame at the terminal's current size, lets `background`
**draw only** (it gets no events while the dialog is up), draws the `dialog`
centred on top through a `child()` sub-canvas, presents, then polls one event and
hands it to the dialog through a fresh [`Context`]. It drains what the dialog
posts exactly as [`Root`] does: a posted command that the dialog declares
*ending* (`Dialog::ends_on`, default `CM_OK`/`CM_CANCEL`) stops the loop and is
returned; any other posted command is re-dispatched into the dialog so a control
can talk to its siblings via the owner chain (ADR 0003). `Esc` makes the dialog
post `CM_CANCEL`; `Enter` activates its default button.

The dialog is **not** spliced into the application's view tree. It is created by
whoever opens it, run by `exec_view`, and dropped when the loop returns — so the
background and the dialog are independent objects with no aliasing. Triggering
(an editor command that wants a dialog) is wired in Phase 6; Phase 5 drives
`exec_view` directly in tests against the scripted terminal.

`MessageBox` is a thin convenience: build a one-label, N-button `Dialog`, call
`exec_view`, return the chosen command.

### Focus-aware drawing: a `set_focused` push

Add one defaulted method to the `View` trait:

```rust
fn set_focused(&mut self, focused: bool) { let _ = focused; }
```

A focusable view that cares about focus stores the flag and consults it in
`draw`; everything else inherits the no-op and is unchanged. The owning `Group`
*pushes* the flag as focus moves — on construction it tells its initial focused
child `set_focused(true)`, and `move_focus` clears the old child and sets the new
one. A `Group` forwards a `set_focused` it receives to its own focused child, so
the signal composes through nested groups.

This is the **same pattern the desktop already uses** for windows
(`Window::set_active`, ADR 0016), promoted to the trait so any control can opt in.
`draw` keeps its `&self`/`Canvas` signature: focus is state the view was told
about earlier, not a draw-time argument.

## Consequences

- **The loop stays single-rooted and the terminal stays owned in one place.**
  `exec_view` reuses `Application`'s terminal and the same draw→present→poll→drain
  shape as `run`; no terminal handle escapes into the tree, and the borrow checker
  is satisfied because background and dialog are disjoint.
- **The menu pull-down can be reconciled (ADR 0016) but need not be yet.** The
  hand-rolled state machine still works; re-expressing it as a modal view is now
  *possible* and left as a later cleanup, not forced.
- **`draw` is untouched across the codebase.** Adding a defaulted trait method is
  source-compatible; existing views and their snapshots are unaffected. Focus
  drawing is now uniform: controls read `self.focused`, set by their owner.
- **Cursor is drawn, not hardware-positioned (yet).** A focused input line paints
  its caret as a styled cell. A real terminal hardware cursor (and the backend
  call to place it) is deferred until the editor needs it (Phase 6); the seam is
  the same `set_focused` flag.
- **Modal gating is coarse for now.** `exec_view` runs the dialog under a fresh
  all-enabled `CommandSet`; per-dialog command disabling (a greyed OK until a
  field is valid) is a later refinement, noted as an open question in the dialog
  spec.
- **Live background, simple resize.** Because `background` is redrawn each frame,
  a resize while a dialog is open relays the background out and re-centres the
  dialog for free; the background receives no events, so it cannot change under
  the dialog.

## Alternatives considered

- **A free `exec_view` function generic over `Backend + EventSource`.** Works, but
  every caller would thread the terminal and timeout by hand; a method on
  `Application` already holds both and reads as the sibling of `run`.
- **Snapshot the background once instead of redrawing it.** Capture the screen
  buffer when the dialog opens and blit it each frame. Simpler, but a resize mid-
  dialog then shows a stale, wrongly-sized backdrop. Redrawing a `Program` costs
  one extra `draw` per frame and keeps resize correct.
- **Thread focus through `draw` (new signature or a `DrawContext`).** Pass focus
  (and later the theme, cursor, clip) as a draw-time context object. Cleaner in
  the abstract, but it rewrites every `draw` in the framework for a need only
  focusable controls have today. Deferred (YAGNI); `set_focused` covers Phase 5,
  and a richer `DrawContext` can subsume it later if the theme/cursor also want in.
- **A separate `Control` trait distinct from `View`.** Give controls their own
  trait with focus state, leaving `View` pure. Rejected: controls *are* views
  (they nest in groups, dispatch the same way); a second trait would duplicate the
  tree plumbing. One defaulted method on `View` is far less surface.
