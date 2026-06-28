# CLAUDE.md

Working guide for this repository. Read `docs/roadmap.md` for *what* we're
building and `docs/adr/` for *why* the big decisions went the way they did.

## What this is

A text-mode editor (`edit`) in the spirit of MS-DOS 6 `EDIT`, built on top of a
hand-rolled TurboVision-style terminal UI framework (`rvision`). It is a Rust
learning project: build as much as practical ourselves; reach for a crate only
at the OS/terminal boundary or for Unicode data tables.

## Layout

- `crates/rvision/` — the UI framework (library). Reusable, no editor knowledge.
- `crates/edit/` — the editor binary, depends on `rvision`.
- `docs/adr/` — one numbered Architecture Decision Record per major decision.
- `docs/roadmap.md` — phased plan; each phase lists modules, an interface
  sketch, and the tests to write first.
- `docs/module-spec-template.md` — copy this before building any new module.

## Non-negotiables (the decisions, in short)

- **Crate budget.** Runtime: `crossterm`, `unicode-width`,
  `unicode-segmentation`. Dev: `insta`. Adding anything else needs a new ADR.
  (ADR 0001, 0006, 0013.)
- **The seam above crossterm.** The framework never calls crossterm directly.
  It draws into an in-memory `Cell` back-buffer; a `Backend` flushes it and an
  `EventSource` supplies events. `CrosstermBackend` for real use, `TestBackend`
  for tests. (ADR 0002.)
- **Retained-mode view tree.** Parent owns children (`Vec<Box<dyn View>>`).
  Views never hold references to each other: commands bubble **up** the owner
  chain, broadcasts travel **down**, identity is via integer IDs. (ADR 0003.)
- **Three-phase events.** Positional → focused → broadcast; modal dialogs run
  via `exec_view`; "handled" is a returned `EventResult`, never a mutated event.
  (ADR 0004.)
- **Colour by role.** Views ask for semantic roles resolved against a `Theme`;
  the cell colour type is truecolour-ready but themes ship 16-colour CGA first.
  (ADR 0005.)
- **Full Unicode.** A cell holds a grapheme cluster + display width; cursor
  movement steps by grapheme. (ADR 0006.)
- **Reversible edits.** Every buffer mutation goes through a reversible `Edit`
  type from the start, even before the undo stack exists. (ADR 0011.)
- **No `unsafe`.** `rvision` sets `#![forbid(unsafe_code)]`; crossterm owns the
  FFI.
- **Panic-safe terminal.** Startup installs an RAII guard + panic hook so a
  crash always restores the terminal (cooked mode, leave alternate screen).
- **Single-threaded sync loop.** `poll(timeout)` → `read()`; the timeout drives
  idle/blink/resize. No async, no tokio.

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

## Commands

```sh
cargo test                 # everything
cargo test -p rvision      # framework only
cargo run -p edit          # run the editor
cargo clippy --all-targets # lints
cargo fmt                  # format
cargo doc --open           # API docs
cargo insta review         # review pending snapshot changes (once insta is wired)
```

## Style

- Match the surrounding code's idiom and comment density. Comments explain
  *why*, not *what*.
- Hand-rolled error types implementing `std::error::Error` — no `thiserror` /
  `anyhow`.
- Keep `rvision` free of any editor-specific concepts; the dependency only
  points one way (`edit` → `rvision`).
