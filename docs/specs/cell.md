# Module spec: `rvision::cell`

- **Status:** In progress
- **Phase:** 1
- **Related ADRs:** 0006 (full Unicode), 0002 (cells are the unit of the screen buffer)

## Purpose

The atom of the screen: one terminal cell holding a grapheme cluster, its display
width in columns, and its [`Style`]. Encapsulates the awkward fact that "one
visible character" can be several Unicode scalars and can occupy one or two
columns.

## Public interface

```rust
pub struct Grapheme(/* Single(char) | Cluster(Box<str>) */);
impl Grapheme {
    fn from_char(c: char) -> Self;
    fn new(s: &str) -> Self;     // one scalar -> Single (no alloc); else Cluster
    fn width(&self) -> u16;      // via unicode-width; control/zero-width -> 0
}
impl fmt::Display for Grapheme  // writes the cluster's text

pub struct Cell { /* grapheme, width (cached), style */ }
impl Cell {
    fn new(grapheme: Grapheme, style: Style) -> Self;
    fn from_char(c: char, style: Style) -> Self;
    fn blank(style: Style) -> Self;       // a space
    fn grapheme(&self) -> &Grapheme;
    fn width(&self) -> u16;
    fn style(&self) -> Style;
}
impl Default for Cell  // blank space, default style
```

## Behaviour & invariants

- A single Unicode scalar is stored inline as `Single(char)` (no heap
  allocation); the blank cell is `Single(' ')`, so clearing a buffer never
  allocates.
- Multi-scalar clusters (e.g. base + combining marks, ZWJ emoji) are stored as
  `Cluster(Box<str>)`.
- `width` is computed once at construction via `unicode-width`: 0 for
  control/zero-width, 1 for normal, 2 for wide (CJK/wide emoji). A two-column
  cell's trailing column is represented by a continuation cell at the buffer
  level (Phase: buffer), not here.
- A `Cell` is a cheap, `Clone`able value; equality compares grapheme + style
  (width is derived, so it follows).

## Collaborators

Depends on `color` (`Style`) and `unicode-width`. Consumed by `buffer` (a grid
of cells) and the backend (renders each cell).

## Test plan (vertical slices)

1. (tracer) `Cell::from_char('A', style)` -> width 1, style preserved, displays "A".
2. A wide scalar ('世') -> width 2.
3. A combining cluster ("e\u{0301}") via `Grapheme::new` -> one grapheme, width 1.
4. Zero-width/control -> width 0.
5. `Cell::default` / `blank` is a single space with the given/default style.

## Open questions

- Width is cached now; if grapheme ever becomes mutable in place, recompute.
