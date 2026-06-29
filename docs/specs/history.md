# Module spec: `edit::history`

- **Status:** Done
- **Phase:** 7 (editing features) — sub-phase 7b
- **Related ADRs:** 0011 (reversible-edit journal)

## Purpose

The editor's undo/redo journal: two stacks of **records**, where each record is
one undoable user action (one or more reversible [`Edit`]s plus the caret before
and after). It coalesces consecutive same-kind keystrokes (a run of typing, a run
of deletes) into a single record, and tracks a "saved" marker so the editor's
dirty flag can clear when the document is undone/redone back to its saved state.

It is *not* the buffer: `History` never holds a document and never applies an
edit. The editor pops a record, applies its edits (forward for redo, inverted for
undo) to its own `LineArray`, then hands the record to the other stack. This keeps
the journal pure and trivially unit-testable (ADR 0011).

## Public interface (crate-internal)

```rust
pub(crate) enum Coalesce { Typing, Deleting, Standalone } // how the next record may merge

pub(crate) struct Record { /* id, before, after, edits, coalesce */ }
impl Record {
    pub(crate) fn before(&self) -> Position;
    pub(crate) fn after(&self) -> Position;
    pub(crate) fn edits(&self) -> &[Edit];
}

pub(crate) struct History { /* undo, redo, next_id, open, saved_id */ }
impl History {
    pub(crate) fn new() -> Self;
    pub(crate) fn record(&mut self, before: Position, edits: Vec<Edit>, after: Position, c: Coalesce);
    pub(crate) fn take_undo(&mut self) -> Option<Record>; // pop top undo; caller applies inverses
    pub(crate) fn push_redo(&mut self, rec: Record);
    pub(crate) fn take_redo(&mut self) -> Option<Record>; // pop top redo; caller re-applies
    pub(crate) fn push_undo(&mut self, rec: Record);      // return a redone record, sealed
    pub(crate) fn break_run(&mut self);                   // a cursor move ends the open run
    pub(crate) fn can_undo(&self) -> bool;
    pub(crate) fn can_redo(&self) -> bool;
    pub(crate) fn is_modified(&self) -> bool;             // top-of-undo id != saved id
    pub(crate) fn mark_saved(&mut self);                  // pin the saved marker, seal the run
}
```

## Behaviour & invariants

- **Recording clears redo.** Any new edit invalidates the redo stack.
- **Coalescing.** A new record merges into the open top record iff the run is
  open, both are single-edit, and the kinds match and abut:
  - *Typing*: the new `Insert` begins exactly where the previous one ended
    (`position_after(at, text)`); merge appends the text, keeping the run's
    original `before` and adopting the new `after`.
  - *Deleting*: backspace (the new `Delete` ends where the old one begins) prepends
    its text and moves `at` left; forward-delete (same `at`) appends its text.
  - A `Standalone` record (Enter, paste, cut, replace-selection, line-join) never
    coalesces and closes the run.
- **`break_run`** closes the open run without touching the stacks, so a cursor
  move splits two typing bursts into separate undo units.
- **Dirty tracking is identity-based, not depth-based.** Each pushed record gets a
  unique id; `mark_saved` pins the current top id (and seals the run). `is_modified`
  is "top-of-undo id ≠ saved id", so undo-to-save clears the flag while *different*
  edits that happen to land at the saved depth still read modified (they carry a
  fresh id). An empty undo stack reads as id `None`.

## Collaborators

- `crate::text` — `Edit` (invert/apply done by the editor), `Position`,
  `position_after` (adjacency test + caret-after of an insert).
- `crate::editor` — the only caller; owns a `History`, applies edits, breaks the
  run on cursor motion, and surfaces `undo`/`redo`/`is_modified`/`mark_saved`.

## Test plan (write these first)

- **Logic:** record→take_undo returns the record; typing coalesces; a break stops
  it; non-adjacent inserts don't merge; backspace and forward-delete each coalesce;
  typing and deleting don't merge; redo replays then a new edit clears redo.
- **Dirty marker:** clean when new; modified after an edit; clean after
  mark_saved; modified after undoing past the save; the depth-collision case still
  reads modified.
- **Integration (in `editor`):** type/undo/redo round-trips; coalesced typing
  undoes as one unit; undo past save clears the modified flag.

## Open questions

- Coalescing across a *replace-selection* is deliberately off (that action is
  `Standalone`). Revisit if it feels heavy in practice.
