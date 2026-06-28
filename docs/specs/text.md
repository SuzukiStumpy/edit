# Module spec: `edit::text`

- **Status:** In progress
- **Phase:** 1 (text-model track)
- **Crate:** `edit` (not `rvision`). The document model is editor-specific —
  lines, cursor positions, a reversible-edit/undo journal — so it lives in the
  application, keeping `rvision` free of editor concepts (CLAUDE.md). TurboVision
  did the same: the editor was a separate unit, not part of the view core.
- **Related ADRs:** 0006 (full Unicode), 0008 (line-array `TextBuffer`),
  0011 (reversible `Edit`)

## Purpose

The editor's in-memory document: an ordered sequence of text lines, edited only
through reversible operations. This is pure logic — no screen, no styling, no
viewport. Rendering a document is the editor view's job (Phase 6); this module
just stores text and mutates it correctly.

What it is *not*: it knows nothing about cursors-as-UI, selections, files, or
encodings. It deals in document positions (line, grapheme-column) and plain
`String` content.

## Public interface

```rust
/// A document position: 0-based line, 0-based grapheme column within that line.
pub struct Position { pub line: usize, pub column: usize }

/// A reversible mutation. `Insert` and `Delete` are exact inverses: each carries
/// the full text involved, so the span affected is derivable from (at, text)
/// alone and no buffer read is needed to invert.
pub enum Edit {
    Insert { at: Position, text: String },
    Delete { at: Position, text: String },
}

impl Edit {
    fn insert(at: Position, text: impl Into<String>) -> Self;
    fn delete(at: Position, text: impl Into<String>) -> Self;
    fn invert(&self) -> Edit;   // Insert<->Delete, same at & text
}

/// The document representation seam (ADR 0008). A gap buffer / rope can be added
/// later behind this trait without touching the editor.
pub trait TextBuffer {
    // Required.
    fn line_count(&self) -> usize;
    fn line(&self, index: usize) -> Option<&str>;
    fn apply(&mut self, edit: &Edit);

    // Provided (depend only on the three required methods).
    fn line_graphemes(&self, index: usize) -> usize;
    fn grapheme_after(&self, pos: Position) -> Option<Position>;
    fn grapheme_before(&self, pos: Position) -> Option<Position>;
    fn to_text(&self) -> String;        // lines joined by '\n'
}

/// Line-array implementation: one UTF-8 `String` per line (ADR 0008).
pub struct LineArray { /* lines: Vec<String> */ }
impl LineArray { fn new() -> Self; }     // also From<&str>, Default
```

## Behaviour & invariants

- **Always ≥ 1 line.** An empty document is one empty line (`[""]`). `From<&str>`
  splits on `'\n'`; a trailing `'\n'` yields a trailing empty line (a file ending
  in a newline). `""` parses to a single empty line.
- **Columns are graphemes,** not bytes or scalars (`unicode-segmentation`,
  ADR 0006). `column == line_graphemes(line)` is the end-of-line position (valid).
- **Insert** splits the target line at `at.column`; embedded `'\n'`s in `text`
  introduce new lines (head+first … middle … last+tail).
- **Delete** removes the span starting at `at` whose shape is given by `text`
  (its newline count and trailing grapheme length), joining the start line's head
  to the end line's tail. `text` is the content that was/will-be removed, so
  `invert` re-inserts exactly it.
- **Reversibility (ADR 0011):** `invert` swaps the variant, keeping `at`/`text`.
  Therefore for any buffer `b` and edit `e`,
  `apply(invert(e), apply(e, b)) == b` — the property test.
- **Navigation** steps by one grapheme, crossing line boundaries: end of line *n*
  → start of line *n+1*; start of line *n* → end of line *n−1*. `grapheme_after`
  returns `None` at document end, `grapheme_before` at document start.
- All accessors are total (out-of-range line → `None` / `0`), never panic.

## Collaborators

`unicode-segmentation` for grapheme boundaries. No dependency on `rvision` at all
— the text model is pure document logic, independent of the screen. Consumed by
the editor view (Phase 6, which bridges it to an `rvision` `Buffer`) and the
undo/redo stacks (Phase 7).

## Test plan (write these first)

- **Logic:** construct (new/From) → line_count/line/to_text round-trip; insert
  within a line; insert with `'\n'` (line split); delete within a line; delete
  spanning lines (join); `line_graphemes` counts graphemes incl. wide/combining.
- **Navigation:** `grapheme_after`/`grapheme_before` within a line and across
  line boundaries; `None` at the document ends.
- **Property:** hand-rolled generator (no proptest — crate budget) producing
  random inserts/deletes; assert `apply` then `apply(invert)` restores the buffer.
- **Manual:** none yet (no UI); exercised for real in Phase 6.

## Open questions

- A range/slice extractor (for selection & clipboard) is deferred to when
  selection lands (Phase 7); the editor can build `Delete` edits from it then.
</content>
</invoke>
