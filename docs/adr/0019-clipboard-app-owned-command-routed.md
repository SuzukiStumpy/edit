# ADR 0019 — The clipboard is app-owned and reached by commands

- **Status:** Accepted
- **Date:** 2026-06-29
- **Phase:** 7 (editing features) — sub-phase 7a

## Context

Phase 7 adds Cut / Copy / Paste. The selection already renders and
[`EditorView::selected_text`](../../crates/edit/src/editor.rs) already exists; the
open question is *where the clipboard lives* and *how an edit reaches it*.

Two forces pull on the answer:

- **No shared references between views ([rvision's ADR 0003](https://github.com/SuzukiStumpy/rvision/blob/main/docs/adr/0003-view-model-trait-objects-messages.md)).** A view never holds a pointer
  to a sibling or to app state; it emits a `Command` that bubbles up the owner
  chain. So the editor cannot simply borrow a clipboard.
- **MDI is coming (Phase 8).** Several editor windows will share *one* clipboard —
  cut here, paste there. A clipboard stored inside one `EditorView` would be the
  wrong home the moment a second document exists.

The system clipboard (OSC 52) is explicitly deferred to Phase 10; Phase 7 ships an
**internal** clipboard only.

## Decision

The clipboard is a `String` **owned by `EditorApp`** (the driver root), not by the
`EditorView`. Cut/Copy/Paste are application commands the editor *posts*; the
driver acts on them:

- `CM_CUT`, `CM_COPY`, `CM_PASTE` are declared by `edit::editor` (it is what emits
  them) in the `CM_USER` range.
- The `EditorView` recognises the clipboard keys (Ctrl+C/X/V and the classic
  Ctrl+Ins / Shift+Ins / Shift+Del) and **posts the command** — it performs no
  clipboard I/O and holds no clipboard state. The Edit menu posts the same
  commands.
- `EditorApp::handle_clipboard` is the one place that touches the clipboard:
  Copy reads `editor.selected_text()`; Cut reads `editor.take_selection()` (return
  + delete in one step); Paste calls `editor.insert_text(&clipboard)`. It needs no
  `Application`/terminal, so it is unit-tested headlessly and runs before the
  dialog-bearing file commands in the driver.

The editor's contribution is two pure, reversible-edit-based methods —
`insert_text` (multi-line paste, replacing any selection, cursor to the far end)
and `take_selection` — both built from the existing `Edit` path (ADR 0011), so cut
and paste are automatically undoable once 7b lands the journal.

## Consequences

- The editor stays clipboard-agnostic: it knows how to *yield* a selection and
  *insert* text, not where bytes are parked. Moving to a shared, multi-window
  clipboard in Phase 8 means relocating one `String`, not rewriting the editor.
- One mutation path holds: paste and cut are ordinary `Edit`s, so selection,
  undo, and find/replace all compose over the same primitive.
- A command round-trip (`post` → driver → `handle_clipboard`) replaces a direct
  call. That is the same indirection menus and buttons already use, and it keeps
  the no-shared-refs invariant intact.

## Alternatives considered

- **Clipboard inside `EditorView`** — simplest now (handle the keys in place,
  store a `String`), but it privatises state that Phase 8 must share, forcing a
  refactor exactly when MDI arrives.
- **Clipboard in the framework (`rvision`)** — cut/copy/paste are generic enough,
  but the buffer model and selection semantics are the editor's; pushing a
  clipboard down would leak editor concepts into a deliberately editor-agnostic
  framework (CLAUDE.md, ADR 0008). Revisit only if a second `rvision` app needs
  text clipboarding.
- **System clipboard now (OSC 52)** — deferred to Phase 10 with the other
  cross-platform polish; an internal clipboard is enough to build and test the
  command routing.
