# Module spec: `rvision::theme`

- **Status:** In progress
- **Phase:** 1
- **Related ADRs:** 0005 (semantic roles over a swappable theme)

## Purpose

Map **semantic roles** (what a piece of UI *is*) to concrete [`Style`]s (how it
looks). Widgets ask for a role and never name colours, so swapping the theme
re-skins the whole UI. Ships one default 16-colour CGA theme.

## Public interface

```rust
pub enum Role { DesktopBackground, WindowFrame, WindowTitle, MenuBar,
                MenuSelected, MenuDisabled, StatusBar, StatusKey,
                ButtonNormal, ButtonFocused, EditorText, Selection }
impl Role {
    pub const ALL: [Role; _];     // every role, in discriminant order
    pub const COUNT: usize;       // == ALL.len()
}

pub struct Theme { /* [Style; Role::COUNT] */ }
impl Theme {
    fn style(&self, role: Role) -> Style;          // lookup
    fn with(self, role: Role, style: Style) -> Self; // override one role
}
impl Default for Theme { /* the CGA palette */ }
```

## Behaviour & invariants

- A `Theme` stores one `Style` per role in an array indexed by `role as usize`;
  **`Role::ALL` must be in discriminant order** so the cast is a valid index
  (guarded by a test).
- `style` is total: every role resolves to a style.
- `with` returns a new theme with a single role replaced, others untouched.
- The default theme uses canonical CGA colours; values are provisional and may be
  tuned as widgets are built.

## Collaborators

Depends on `color` (`Style`, `Color`, `Color16`). Consumed by every widget when
drawing (a `&Theme` will be threaded through draw in later phases).

## Test plan (vertical slices)

1. (tracer) default theme resolves a few known roles to expected CGA styles.
2. `Role::ALL` is in discriminant order and `COUNT == ALL.len()` (index safety).
3. `with` overrides one role and leaves the rest unchanged.

## Open questions

- The role set will grow as widgets arrive; additions append to `ALL` and the
  array resizes via `COUNT`.
