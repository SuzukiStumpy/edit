# CLAUDE.md

Working guide for this repository. Read `docs/roadmap.md` for *what* we're
building and `docs/adr/` for *why* the big decisions went the way they did.

## What this is

A text-mode editor (`edit`) in the spirit of MS-DOS 6 `EDIT`, built on top of a
hand-rolled TurboVision-style terminal UI framework,
[`rvision`](https://github.com/SuzukiStumpy/rvision) — its own repository since
it outgrew living alongside the editor. It is a Rust learning project: build as
much as practical ourselves; reach for a crate only at the OS/terminal boundary
or for Unicode data tables.

## Layout

- `crates/edit/` — the editor binary. Depends on `rvision` as a git dependency
  (see its `Cargo.toml`); copy `.cargo/config.toml.example` to
  `.cargo/config.toml` (git-ignored) to `[patch]` it to a local sibling
  checkout during active co-development, without affecting CI.
- `docs/adr/` — one numbered Architecture Decision Record per major decision.
  Decisions about the framework itself now live in `rvision`'s own `docs/adr/`;
  citations here to a moved ADR link across to it.
- `docs/roadmap.md` — phased plan; each phase lists modules, an interface
  sketch, and the tests to write first.
- `docs/module-spec-template.md` — copy this before building any new module.

## Non-negotiables (the decisions, in short)

- **Crate budget.** `edit` itself adds only `unicode-segmentation` (grapheme
  navigation in the text model) and, for tests, `insta`. `crossterm` and
  `unicode-width` are `rvision`'s runtime deps, reached only through it.
- **Reversible edits.** Every buffer mutation goes through a reversible `Edit`
  type from the start, even before the undo stack exists. (ADR 0011.)
- Everything about how `rvision` itself works — the crossterm seam, the
  retained-mode view tree, three-phase events, colour-by-role, full Unicode,
  `#![forbid(unsafe_code)]`, the panic-safe terminal guard, the single-threaded
  sync loop — is now `rvision`'s own set of non-negotiables; see its `CLAUDE.md`
  and `docs/adr/`. `edit`'s own driver loop (ADR 0018) follows the same
  single-threaded, no-async shape by choice, not by inheriting the constraint.

## How we work

- **TDD, always.** Red → green → refactor. Write the failing test first.
  - *Logic* (geometry, buffers, dispatch, `Edit` apply/invert): plain `#[test]`.
    The `Edit` invariant `invert(apply(x)) == x` is a property test.
  - *Rendering*: draw into a `TestBackend` and assert with `insta` snapshots.
  - *Interaction*: feed a scripted event sequence, assert screen + model state.
  - *Manual*: `examples/` demos and running `edit` for real (colours, feel).
- **Per-module process.** Copy `docs/module-spec-template.md`, fill it in
  (purpose, public interface, invariants, test list), *then* write tests, *then*
  code. Record any design decision worth keeping as a new ADR.
- **Docs.** Rustdoc on every public item (`#![warn(missing_docs)]` is on).
  Keep the relevant ADR/roadmap entry updated when a decision or plan changes.
- **Commits.** Conventional Commits — `feat(edit): …`, `fix(edit): …`,
  `feat(…)!:` / a `BREAKING CHANGE:` footer for a major bump. release-please
  reads these to compute the next version (ADR 0024); an unscoped or
  non-conventional subject just won't drive a bump.

## Commands

```sh
cargo test                 # everything
cargo run -p edit          # run the editor
cargo clippy --all-targets # lints
cargo fmt                  # format
cargo doc --open           # API docs
cargo insta review         # review pending snapshot changes
```

## Style

- Match the surrounding code's idiom and comment density. Comments explain
  *why*, not *what*.
- Hand-rolled error types implementing `std::error::Error` — no `thiserror` /
  `anyhow`.
- `rvision` stays free of any editor-specific concepts — now enforced by being
  a separate repository, not just convention.
