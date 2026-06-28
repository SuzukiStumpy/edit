# ADR 0012 — Workspace layout, Rust 2024, pinned MSRV, hand-rolled errors

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

The stated goal is "build the UI library first, then the editor on top," which
argues for a clean separation between framework and application. We also need to
settle low-controversy toolchain defaults once.

## Decision

A **Cargo workspace** with two member crates:

- `crates/rvision/` — the TurboVision-style UI framework (library), with no
  editor knowledge; an `examples/` area hosts demos used for manual verification.
- `crates/edit/` — the editor binary, depending on `rvision`.

The dependency points one way only (`edit` → `rvision`). Toolchain: **Rust 2024
edition**, **MSRV pinned** via `rust-toolchain.toml` (build on the MSRV to keep it
honest), **hand-rolled error types** implementing `std::error::Error` (no
`thiserror`/`anyhow`).

## Consequences

- `rvision` is a standalone, independently testable, reusable artifact.
- A clean layering boundary, enforced by the crate split.
- A little Cargo ceremony vs a single crate; far less than a fine-grained
  multi-crate split.

## Alternatives considered

- **Single crate, modules** — least ceremony, but the framework/app boundary is
  only a convention and the "library" isn't a real artifact.
- **Fine-grained workspace** (geometry/screen/widgets/app as separate crates) —
  maximum modularity, but more structure and inter-crate juggling than needed now.
