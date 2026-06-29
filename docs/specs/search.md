# Module spec: `edit::search`

- **Status:** Done
- **Phase:** 7 (editing features) — sub-phase 7c.2
- **Related ADRs:** 0006 (full Unicode), 0008 (line-array text)

## Purpose

Buffer text search: locate a needle within the document and return the matching
span. Pure logic over any [`TextBuffer`](crate::text::TextBuffer) — it never
mutates and holds no editor state — so it is unit-tested against a `LineArray`
alone. Search is **line-oriented** (the needle carries no newline), matching the
spirit of MS-DOS `EDIT`; positions are `(line, grapheme-column)` so a [`Match`]
drops straight into the editor's selection.

## Public interface

```rust
pub struct Query { pub needle: String, pub case_sensitive: bool, pub whole_word: bool }
impl Query {
    pub fn new(needle: &str) -> Self;          // case-insensitive, not whole-word
    pub fn find(&self, buf: &impl TextBuffer, from: Position, backward: bool, wrap: bool)
        -> Option<Match>;
}
pub struct Match { pub start: Position, pub end: Position } // half-open, same line
```

## Behaviour & invariants

- **Direction + wrap.** Forward scans from `from.column` on the start line, then
  whole following lines; `wrap` appends the lines before `from` and finally the
  earlier part of the start line, so a Find Next sweeps the whole document exactly
  once. Backward mirrors this, picking the *last* match in each window.
- **Case folding** by `to_lowercase` per grapheme when `!case_sensitive`.
- **Whole word** requires a non-word grapheme (or the line edge) on each side of
  the match; a word grapheme is alphanumeric or `_`.
- **Grapheme columns, not bytes.** Matching steps by grapheme cluster, so a match
  after a wide/combining grapheme reports the right column.
- **Empty needle / no match → `None`.**

## Collaborators

- `crate::text` — `TextBuffer::line`/`line_count`, `Position`.
- `crate::editor` — owns a `last_query`, calls `find` from the caret (forward) or
  selection start (backward), selects the [`Match`], and reveals it; `find_next`
  repeats. The Find dialog (`edit::dialogs`) builds the `Query`.

## Test plan (done)

- **Logic:** match within a line; case sensitivity; whole-word rejects substrings;
  forward across lines; forward/backward wrap; backward picks the previous match;
  empty needle / no match; grapheme (not byte) columns.

## Open questions

- Multi-line / regex search is out of scope (line-oriented, literal needle), in
  keeping with the MS-DOS `EDIT` model; revisit behind a new ADR if needed.
