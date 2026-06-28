# edit

A text-mode editor in the spirit of MS-DOS 6 `EDIT`, built on a hand-rolled
[TurboVision](https://en.wikipedia.org/wiki/Turbo_Vision)-style terminal UI
framework. Cross-platform (Linux primary; Windows and macOS supported).

This is a Rust learning project: as much as practical is built from scratch.
The only external runtime crates are the OS/terminal boundary and the Unicode
data tables that the standard library doesn't ship.

## Layout

```
crates/
  rvision/   the TurboVision-style UI framework (library)
  edit/      the editor binary, built on rvision
docs/
  roadmap.md            phased delivery plan
  adr/                  architecture decision records
  module-spec-template.md
```

## Build & test

```sh
cargo build            # whole workspace
cargo test             # all tests (unit + snapshot + interaction)
cargo run -p edit      # run the editor
cargo doc --open       # browse the API docs
```

The toolchain is pinned in `rust-toolchain.toml` (MSRV 1.85, Rust 2024 edition).
If you don't have Rust, install it via [rustup](https://rustup.rs).

## Where to start reading

`docs/roadmap.md` for the plan, then `docs/adr/` for *why* each major design
decision was made. `CLAUDE.md` holds the working conventions.
