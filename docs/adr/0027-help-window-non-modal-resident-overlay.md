# ADR 0027 — The help window is a non-modal, resident, standalone overlay

- **Status:** Accepted
- **Date:** 2026-07-06

## Context

ADR 0026 made `open_help` build `rvision::widgets::HelpWindow` and run it
through `app.exec_view`, matching every other editor dialog. That was wrong
for this specific window: `HelpWindow::build`/`build_at` return a plain
`Window` with `Window::new`'s fully-capable defaults — resizable, moveable,
closable, zoomable — so the chrome *draws* a resize handle, zoom glyph, and
close glyph. `exec_view`'s modal loop never wires up drag/resize at all (that
machinery lives in `rvision::widgets::Desktop`, which `edit` doesn't use —
ADR 0018), and only a command in `Window::ending` ends the loop (just
`CM_CANCEL`, from `.also_ends_on`). Clicking any of those glyphs did nothing.
`HelpWindow::build`'s own doc comment already said as much: it's "meant to be
opened non-modally via `Desktop::open`, not run through
`Application::exec_view`."

The fix is to make it behave like what it visually presents as: movable,
resizable, and closable for real, with no modal status — while keeping the
zoom glyph off (not needed). Since `edit` doesn't use `rvision::Desktop`, this
means hosting the help window as a second kind of resident window inside
`EditorApp`'s own hand-rolled MDI (`app.rs`'s `chrome_hit`/`Drag`/z-order
machinery, which already manages `Document` windows) rather than handing it
to `Desktop`.

**Scope, chosen explicitly over full MDI integration:** the help window is a
**standalone singleton overlay** — its own move/resize/close state,
click-to-front against documents — but it does **not** join the Window menu,
F6 cycle, or Alt+1..9; those stay document-only. Full integration would mean
generalizing `documents: Vec<Document>` to hold a second window kind and
reworking every document-only command (Save/Cut/Copy/Find/etc.) to behave
sensibly when the active window isn't a document — a much larger change for a
single utility window with no file to save.

**Verified against `rvision`'s actual source** (`src/widgets/window.rs`):

- `Window::handle_event`'s `Event::Mouse` arm *does* detect its own
  close/zoom-glyph clicks and self-post `CM_CLOSE`/`CM_ZOOM` — but only past
  the point where a host would already have intercepted a title-bar/corner
  press for a drag session; `Window` "has no concept of a drag session" by
  design (that's `Desktop`'s job, ADR 0016 in `rvision`). `edit` doesn't use
  this path at all: its own `chrome_hit`/`Frame::close_span`/`zoom_span`
  already do the identical geometry test for `Document` windows, generalized
  and reused directly for help — a chrome click is never forwarded into
  `Window::handle_event`, exactly matching how a `Document`'s chrome works.
- `Window::bounds()`/`set_bounds()`, and everything `Window::draw`/
  `handle_event` do internally, use the **same "desktop-local" coordinate
  convention** `edit` already uses for `Document.normal`: `Canvas::child(rect)`
  resets the child canvas's own `(0,0)` to `rect`'s origin, and `Window` never
  reads its own `bounds.origin()` when drawing, only its size. Help's `Rect`
  bookkeeping and draw/hit-test plumbing are a drop-in parallel to a
  `Document`'s — no coordinate-space translation surprises.
- `Window` already handles its own scroll-bar hit-testing/thumb-dragging for
  its interior — help needs no `ScrollThumb`-equivalent drag variant, only
  `Move`/`Resize`.

## Decision

`EditorApp` gains `help: Option<HelpOverlay>` (`HelpOverlay` wraps the
`rvision::widgets::Window` `HelpWindow::build`/`build_at` returns, plus its
own small `drag: Option<HelpDrag>` — `Move`/`Resize` only) and `help_focused:
bool` (meaningful only while `help` is `Some`; which plane — help or the
document stack — currently owns the keyboard and draws on top).

- **Opening/focusing** (`EditorApp::open_help`, replacing the old free
  `open_help` function): builds the window once — `.moveable(true)
  .resizable(true).closable(true).zoomable(false)` — the *first* time it's
  called; every call after that just focuses the existing one, ignoring
  `initial` (a singleton, reused rather than rebuilt, matching `rvision`'s
  own "hold a window by value" idiom, ADR 0016). No longer needs
  `Application<T>`/`io::Result` since it isn't `exec_view`d.
- **Chrome hit-testing**: `chrome_hit`'s pure-`Rect` geometry (no `Document`
  access at all) split into `chrome_hit_at(rect, pos, zoomable)`, reused
  as-is for help with `zoomable: false`.
- **Closing**: `close()`'s existing discard-guard path now branches first on
  `ed.help_focused()` — a focused help window closes directly
  (`close_help()`), no `confirm_discard` prompt, since there's nothing to
  save. This also makes Alt-F3 close help for free, since it already posts
  `CM_CLOSE` unconditionally.
- **Drawing**: help draws in the same nested desktop canvas documents use,
  either before or after the document loop depending on `help_focused` — a
  deliberate **two-plane simplification** (help is either fully on top of or
  fully behind the whole document stack) rather than interleaving it into
  per-document z-order. A document's active-frame styling is suppressed while
  help has focus, for visual parity with where the keyboard actually goes.
- **Mouse**: help gets first refusal on a press when it's the topmost plane;
  otherwise documents get it, falling back to help only if the press missed
  every document. Move/resize drag their own `HelpDrag` session
  (`start_help_move`/`start_help_resize`/`drag_help_to`), structurally
  identical to a document's but against `window.bounds()`/`set_bounds()`
  instead of `Document.normal` — kept as a small parallel implementation
  rather than generalizing `Drag`/`drag_to`, since help has no zoom and no
  `ScrollThumb` case to thread through. A chrome-`None` hit or a wheel event
  over help's rect forwards into `window.handle_event`, translated to its own
  local coordinates (list selection, page scroll, and the composite's
  internal scroll-bar thumb-drag are all handled entirely by `Window`/its
  interior).
- **Keyboard**: while help is focused, `Esc` closes it directly (`Window` is
  built *without* `.esc_cancels(true)` this time — that was `exec_view`-only
  plumbing); any other key forwards into `window.handle_event`; the active
  document never sees a key while help owns the keyboard, even one help's own
  interior left unhandled. `activate()` (which F6/Shift-F6/Alt+N all call)
  hands focus back to the newly-activated document as a courtesy.
- **Resize**: `relayout` clamps help's bounds to the new desktop size, the
  same way a document's `.normal` is clamped.

## Consequences

- The help window is now honestly what it looks like: draggable by its title
  bar, resizable from its corner, closable by its glyph or Alt-F3/Esc, and
  never blocks interacting with the rest of the editor.
- **Accepted gap**: because help is either fully on top of or fully behind
  the document stack (not interleaved per-window), and `edit`'s first
  document window always covers the entire desktop (whether zoomed, tiled,
  or freshly cascaded), an *unfocused* help window is usually fully obscured
  and mouse-unreachable at its old screen position once a document is
  clicked. `F1` (or Help ▸ Help Topics) is the reliable way back — it always
  focuses the existing window regardless of visibility, so this isn't a dead
  end, but it does mean "click through to the help window peeking out from
  behind" mostly doesn't come up in practice.
- **Accepted gap**: the old hand-rolled `HelpViewer` drew a bottom hint row
  ("↑↓ Topic Tab Switch pane Esc Close"). `rvision::widgets::HelpWindow`'s
  composite interior doesn't render one, and its constructor is private —
  `edit` only ever gets the fully-wrapped `Window` back, with no way to
  compose an extra row around the interior itself. Restoring this needs a
  small additive change in `rvision` (e.g. a public way to reserve a hint
  row). Not attempted here; a possible future `rvision` issue, not a
  regression introduced by this ADR (ADR 0026 already lost it).
- The bigger Desktop/Window convergence question (accelerators, full MDI
  parity, a `status_text`/`StatusPanel` indicator) remains open and
  untouched — this ADR resolves it only for the one case that needed it now.

## Alternatives considered

- **Full MDI integration** (join the Window menu/F6/Alt+N). Rejected for now
  — see Scope above; explicitly the user's choice when this was scoped,
  favouring a smaller blast radius over completeness.
- **Get `rvision` to expose the help composite's bare interior** (its
  constructor is currently private, only `build`/`build_at` are `pub`, and
  both return the fully-wrapped `Window`), so `edit` could compose it inside
  its *own* `Frame`/chrome instead of hosting `rvision`'s `Window` value
  directly. Rejected as unnecessary: `Window` already does everything needed
  (its own draw, its own interior dispatch, its own internal scroll-bar
  handling) once `edit` supplies the title-bar/corner drag-session logic it
  was always going to own anyway (the same `chrome_hit` a `Document` uses) —
  no `rvision` change needed to get a fully working result.
- **Interleave help into per-document z-order** (a unified stack ordering
  help and documents together) instead of the two-plane simplification.
  Rejected as more machinery than a single utility window warrants; revisit
  only if a second non-document overlay appears and the two-plane model
  stops being adequate.
