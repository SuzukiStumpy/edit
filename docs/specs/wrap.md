# Module spec: `rvision::wrap`

- **Status:** In progress
- **Phase:** 10 (polish — dialog/help text)
- **Related ADRs:** 0006 (full Unicode — display width), 0015 (grapheme/width
  logic lives in one place), 0022 (MessageBox splits on `\n`; callers pre-wrap)

## Purpose

A pure text helper: break prose into lines that fit a maximum display width, so
callers (`MessageBox`, the coming Help viewer) pass *paragraphs* instead of
pre-broken lines (the ADR 0022 follow-up in the Backlog). It deals in plain
`String`s — no cells, no styling, no view geometry.

What it is *not*: it does not justify, hyphenate, or know about screen
coordinates; it does not draw anything.

## Public interface

```rust
/// Wrap `text` to at most `width` display columns per line, breaking on ASCII
/// spaces. Returns the wrapped lines, without trailing newlines.
pub fn wrap(text: &str, width: u16) -> Vec<String>;
```

## Behaviour & invariants

- **Hard breaks survive.** Existing `'\n'` always starts a new line; a blank
  line stays a blank line (the spacer rows MessageBox relies on, ADR 0022).
- **Soft breaks fall on spaces.** Within a hard line, words are greedily packed:
  a word joins the current line if the result is `<= width` display columns,
  otherwise it starts the next line. Runs of spaces between words collapse to a
  single separator (standard prose wrapping).
- **Width is display columns** (unicode-width), so a CJK/wide run counts double
  and the result fits a fixed-width box exactly.
- **Over-long words overflow, never split.** A single word wider than `width`
  occupies its own line and is allowed to exceed it, rather than splitting a
  grapheme cluster mid-character. `width == 0` degenerates to one word per line.
- **Empty input** yields one empty line (`[""]`), matching `"".split('\n')`.

## Collaborators

- `unicode_width::UnicodeWidthStr` for the column measurement — the same crate
  the `Cell`/`Buffer` width path uses (ADR 0006/0015).
- Consumed by `widgets::message_box` (and later the Help viewer); produces input
  for their per-line `Label`s.

## Test plan (write these first)

- **Logic:** short text passes through unchanged; a long line breaks on spaces
  at the width boundary; hard `'\n'` and blank lines are preserved; an over-long
  word lands alone and overflows; wide (CJK) text breaks by columns not chars;
  empty string → `[""]`.
- **Property:** every returned line is `<= width` columns *unless* it is a single
  word that alone exceeds `width`.
- **Manual:** the About box and a long MessageBox read naturally in `edit`.

## Open questions

- None outstanding. Word-internal breaking (very long URLs) is deferred until a
  caller needs it.
