# Module spec: `rvision::color`

- **Status:** In progress
- **Phase:** 1
- **Related ADRs:** 0005 (semantic roles over a truecolour-ready type)

## Purpose

Represent colours and text styling in a way that is **truecolour-ready from day
one** but ships 16-colour CGA values first. Pure value types; no terminal I/O and
no role/theme logic (that lives in `theme`).

## Public interface

```rust
pub enum Color16 { Black, Blue, Green, Cyan, Red, Magenta, Brown, LightGray,
                   DarkGray, LightBlue, LightGreen, LightCyan, LightRed,
                   LightMagenta, Yellow, White }
impl Color16 { fn to_rgb(self) -> (u8, u8, u8); }   // canonical CGA values

pub enum Color { Default, Named(Color16), Rgb(u8, u8, u8) }  // Default = terminal default
impl Color { fn resolve_rgb(self) -> Option<(u8, u8, u8)>; } // Default -> None

pub struct Attributes(/* u8 bitset */);
impl Attributes {
    const NONE/BOLD/DIM/ITALIC/UNDERLINE/REVERSE/BLINK: Self;
    fn empty()/is_empty(self)/contains(self, other)/union(self, other);
    // BitOr for ergonomic `BOLD | UNDERLINE`
}

pub struct Style { pub fg: Color, pub bg: Color, pub attrs: Attributes }
impl Style { fn new(); fn fg(self, Color); fn bg(self, Color); fn attrs(self, Attributes); }
```

## Behaviour & invariants

- The 16 named colours store their **canonical CGA RGB** so the aesthetic is
  identical whether a backend emits 16-colour or truecolour escapes.
- `Color::Default` means "the terminal's default" and has no fixed RGB →
  `resolve_rgb` returns `None`.
- `Attributes` is a bitset: `union` combines, `contains(x)` is true only if all
  bits of `x` are set; `NONE`/`empty()` contains nothing.
- `Style` default is `Default` fg/bg with no attributes; builder methods are
  chainable and return a new `Style`.

## Collaborators

Consumed by `theme` (roles → `Style`), by `cell` (each cell carries a `Style`),
and by the backend (which turns `Color`/`Attributes` into escape sequences in
Phase 2). Depends on nothing.

## Test plan (vertical slices)

1. (tracer) `Color16::to_rgb` returns canonical CGA values (spot-check several).
2. `Color::resolve_rgb`: Named via Color16; Rgb returns itself; Default -> None.
3. `Attributes`: empty contains nothing; union + contains; BitOr sugar.
4. `Style`: default is blank; chained builders set fg/bg/attrs.

## Open questions

- ANSI 0..15 index mapping for `Color16` is deferred to the backend (Phase 2).
