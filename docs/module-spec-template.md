# Module spec: <module path, e.g. `rvision::widgets::button`>

> Copy this file to `docs/specs/<module>.md`, fill it in, and get it to a
> coherent state **before** writing tests or code (ADR 0014). Keep it short —
> a page is plenty. Update it if the design shifts during implementation.

- **Status:** Draft | In progress | Done
- **Phase:** <roadmap phase>
- **Related ADRs:** <e.g. 0003, 0004>

## Purpose

One or two sentences: what this module is responsible for, and — just as
importantly — what it is *not*.

## Public interface

Sketch the types and the signatures callers will use. Keep the surface minimal;
prefer adding later over removing.

```rust
// pub struct Button { ... }
// impl Button {
//     pub fn new(label: &str, command: Command) -> Self;
// }
// impl View for Button { ... }
```

## Behaviour & invariants

- What must always hold true? (e.g. "a disabled button never emits its command";
  "width is recomputed whenever the grapheme changes".)
- Edge cases worth naming (empty input, zero-size bounds, off-screen draw,
  wide/combining graphemes, EOL at end of file...).

## Collaborators

Which other modules/traits it uses (e.g. `Buffer`, `Theme`, `EventResult`) and
how it talks to siblings (commands up / broadcasts down — never direct refs).

## Test plan (write these first)

- **Logic:** ...
- **Render (snapshot):** ...
- **Interaction (scripted events):** ...
- **Property (if applicable):** ...
- **Manual:** what to check by eye in an `examples/` demo or in `edit`.

## Open questions

Anything unresolved. Resolve before "Done", or spin out an ADR.
