# ADR 0018 — The editor uses a bespoke driver loop, not `Application::run` + `Root`

- **Status:** Accepted
- **Date:** 2026-06-29
- **Phase:** 6 (editor, single document)

## Context

Phase 6 must wire menu/status commands (New, Open, Save, Save As, Exit) to the
editor, and Open/Save/"save changes?" need **modal file dialogs and message
boxes**. The framework runs modals through
[`Application::exec_view`](../../crates/rvision/src/app.rs), which owns the
terminal and is therefore a method on `Application` — it cannot be called from
inside the view tree (a view holds no terminal, by design — [rvision's ADR 0001](https://github.com/SuzukiStumpy/rvision/blob/main/docs/adr/0001-terminal-backend-crossterm.md)/[0002](https://github.com/SuzukiStumpy/rvision/blob/main/docs/adr/0002-render-seam-backend-double-buffer.md)).

The generic main loop, `Application::run(&mut Root)`, drains the commands the
tree posts and only special-cases `CM_QUIT`; it gives the application no hook to
say "this posted command needs a modal" and no way to reach back into
`Application` to run one. Two further frictions:

- the editor view is owned, type-erased, several layers deep
  (`Shell → Desktop → Window → Box<dyn View>`); the framework deliberately has no
  downcast or view IDs yet (rvision's ADR 0003), so the app could not reach *its own*
  document to load/save into it through `Shell`;
- threading `exec_view` back into `Root`'s command drain would push
  editor-specific control flow into the framework, which must stay editor-agnostic
  (CLAUDE.md, ADR 0008).

## Decision

The `edit` binary drives its **own** loop (`edit::app::run`) over an
`Application<T>` it owns:

```
draw → present → poll → dispatch(event) → for each posted command: act
```

- `edit::app::EditorApp` owns the chrome (`MenuBar`, `StatusLine`) and the
  `EditorView` **concretely**, plus the current file's path/[`Encoding`]. It is
  the layout + key-routing root *and* implements `Program`, so it doubles as the
  `exec_view` background (which only ever draws it).
- `dispatch` runs an event through the three local passes (menu → editor →
  status) and **returns the posted commands** to the driver, which the framework's
  `Program::handle_event` can't surface.
- File commands are handled in the driver, where both `&mut Application` (for
  `exec_view`) and `&mut EditorApp` (the concrete editor) are in scope: Open/Save
  As run a `FileDialog`, "save changes?" runs a `MessageBox`, then the driver
  calls `EditorApp::open_file`/`save_to` directly.

This keeps `Application::run`/`Root` as the simple path for command-only apps
(the chrome demo still uses it) and confines the editor's modal-interleaving
control flow to the editor crate. Because the app owns the editor concretely,
there is **no shared `Rc<RefCell>`, no downcast, and no view IDs**.

## Consequences

- The editor does not reuse `Shell`/`Desktop`/`Window`; it re-implements the
  small three-region layout + menu-overlay routing (≈ Shell, ~40 lines) so it can
  hold the editor concretely. Acceptable duplication for a downcast-free design.
- The file-command effects live in the driver and are exercised by hand (running
  `edit`); the *pieces* they call — `EditorApp::open_file`/`save_to`/`new_file`
  and `dispatch` — are unit-tested without a terminal.
- A future phase (MDI, Phase 8) may lift a reusable "modal-capable root" into the
  framework; until a second app needs it, that abstraction would be speculative.

## Alternatives considered

- **`Application::run` + `Root`, reach the editor via downcast/IDs** — needs
  `Any` on `View` or an ID registry the framework has so far avoided (rvision's ADR 0003),
  and still no `exec_view` hook. **rvision's ADR 0036 supersedes this bullet's
  downcast rejection specifically**: with hindsight (a real `Desktop`, ADR
  0033's shared `arrange` module, and `rvision` now its own repository),
  `rvision` added `Any`-based downcast access to a `Window`'s interior
  content. This ADR's own headline decision — the bespoke driver loop, for
  the still-unrelated `exec_view`-reachability problem — is untouched;
  whether `edit` actually migrates its document MDI onto `Desktop`/`Window`
  is a separate, later decision that downcast alone doesn't force.
- **Shared `Rc<RefCell<Document>>` between the view and the app** — preserves
  `Shell` reuse but splits the editor's state into an interior-mutable cell, a
  smell the codebase has kept out of production code.
- **An in-tree modal layer** (modal view pushed on the desktop, no `exec_view`) —
  a larger framework feature; revisit if the editor outgrows `exec_view`.
