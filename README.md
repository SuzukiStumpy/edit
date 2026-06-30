# edit

A text-mode editor in the spirit of MS-DOS 6 `EDIT`, built on a hand-rolled
[TurboVision](https://en.wikipedia.org/wiki/Turbo_Vision)-style terminal UI
framework. Cross-platform (Linux primary; Windows and macOS supported).

This is a Rust learning project: as much as practical is built from scratch.
The only external runtime crates are the OS/terminal boundary and the Unicode
data tables that the standard library doesn't ship.

## Download

Pre-built binaries for each release are on the
[Releases page](https://github.com/SuzukiStumpy/edit/releases), one per platform:

| Platform | Asset |
|----------|-------|
| Linux (x86-64) | `edit-vX.Y.Z-x86_64-unknown-linux-gnu` |
| Windows (x86-64) | `edit-vX.Y.Z-x86_64-pc-windows-msvc.exe` |
| macOS (Apple Silicon) | `edit-vX.Y.Z-aarch64-apple-darwin` |
| macOS (Intel) | `edit-vX.Y.Z-x86_64-apple-darwin` |

The binaries are unsigned. On Linux/macOS make the file executable
(`chmod +x edit-…`); on macOS you may also need to clear the quarantine flag
(`xattr -d com.apple.quarantine edit-…`) or open it once via right-click → Open.

## Layout

```
crates/
  rvision/   the TurboVision-style UI framework (library)
  edit/      the editor binary, built on rvision
docs/
  roadmap.md            phased delivery plan
  adr/                  architecture decision records
  releasing.md          how releases are cut (maintainers)
  module-spec-template.md
```

## Configuration

**Edit ▸ Settings** sets the tab width and how many recent files the File menu
shows, with a **Reset to defaults** that also clears the otherwise-transparent
preferences (the Find options). The choices persist between sessions in a small
`key = value` file `edit` writes automatically — you can hand-edit it too, and
deleting it resets to defaults:

| OS | Location |
|----|----------|
| Linux/BSD | `$XDG_CONFIG_HOME/edit/config` (else `~/.config/edit/config`) |
| macOS | `~/Library/Application Support/edit/config` |
| Windows | `%APPDATA%\edit\config` |

Keys: `tab_width`, `recent_limit`, the Find `find.case_sensitive` /
`find.whole_word` options, and the `recent` file list. Set `EDIT_CONFIG_PATH` to
point `edit` at a different file. See ADR 0025 for the design.

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
decision was made. `CLAUDE.md` holds the working conventions, and
`docs/releasing.md` is the runbook for cutting a release.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
