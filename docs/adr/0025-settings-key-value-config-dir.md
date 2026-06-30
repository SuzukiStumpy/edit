# ADR 0025 — Settings: hand-rolled key-value file in the platform config dir

- **Status:** Accepted
- **Date:** 2026-06-30

## Context

Phase 10 calls for persisting a few user preferences across runs: tab width, the
recent-files (MRU) list and its length, and the Find dialog's case/whole-word
options. Two things were left to settle when the phase arrived (roadmap "Deferred
decisions"): the on-disk **format** and the file's **location**. An in-app
**Edit ▸ Settings** dialog exposes the two knobs worth a control (tab width and
MRU length) plus a **Reset to defaults**; the rest stay transparent (persisted
automatically, with no UI of their own) — which is why a reset that also restores
those transparent ones is worth having.

The forces:

- **Crate budget (ADR 0001).** Runtime deps are frozen at `crossterm`,
  `unicode-width`, `unicode-segmentation`; anything else needs an ADR. That rules
  out `serde`/`toml` for the format *and* `dirs`/`directories` for the location —
  the two crates one would normally reach for here.
- **It is a Rust learning project (CLAUDE.md).** Hand-rolling a tiny parser and
  per-OS path resolution is squarely the point, not a burden to avoid.
- **The data is trivially small** — three scalars and a short list. It does not
  need a nested document format.
- **Forward/hand-edit safety.** A user might hand-edit the file, and a future
  `edit` will add keys; neither should brick startup on an older or messy file.
- **Testability (ADR 0013).** Path resolution and I/O must be unit-testable
  without depending on the host's real home directory or touching a fixed path.

## Decision

**A hand-rolled `key = value` text format**, parsed leniently, in a file named
`config` under the conventional per-OS configuration directory, resolved by hand
from environment variables:

- Linux/BSD: `$XDG_CONFIG_HOME/edit/config`, falling back to
  `$HOME/.config/edit/config`.
- macOS: `$HOME/Library/Application Support/edit/config`.
- Windows: `%APPDATA%\edit\config`.

The format: one `key = value` per line, `#` comments, blank lines ignored, the
list field (`recent`) repeating its key newest-first. Parsing is **total and
lenient** — unknown keys, malformed lines, and bad values are dropped and the
field keeps its default; it never errors. A missing file loads as `Default`; if
no config dir can be resolved (the env vars are unset), the path is `None`, load
returns `Default`, and save is a silent no-op rather than inventing a location.

The decision lives in a new `edit::settings` module (not `rvision` — it is
editor-specific state, and the framework stays editor-agnostic, ADR 0018). The
format (`parse`/`render`) and the OS resolution (`config_path_for(os, get_env)`)
are split into pure, injectable functions so both are unit-tested for every OS
and round-trip without a filesystem.

## Consequences

- **Easier:** zero new dependencies; a one-screen parser; per-OS path logic and
  the format are fully unit-tested via injected env + temp dirs; a hand-editable,
  greppable file; adding a key later is backward-compatible by construction
  (lenient parse ignores what it doesn't know, and absent keys fall to default).
- **Harder / lived-with:** we own the parser's edge cases (we accept a flat
  key-value shape — no nesting, no types beyond int/bool/path/list); `recent`
  stores absolute paths as plain strings, so a moved/renamed file simply drops off
  the list when it fails to open. Path resolution mirrors the common-case logic of
  `dirs` but not its every corner (e.g. no Windows `known-folder` API call — we
  read `%APPDATA%`); acceptable for a single-user TUI editor.
- **Escape hatch:** the format is versionless but additive; if it ever outgrows
  flat key-value (nested groups, richer types), a `version = N` key can gate a new
  parser without breaking old files. Window-layout persistence was deliberately
  left out of v1 and can be added as new keys.

## Alternatives considered

- **`serde` + TOML/JSON/RON.** The obvious real-world choice; rejected on the
  crate budget and because the payload is too small to justify it — and writing
  the parser is the exercise.
- **`dirs`/`directories` for the path.** Same crate-budget rejection; the per-OS
  rules we need are a dozen lines of `cfg!` + env reads.
- **A dotfile in `$HOME` (`~/.editrc`) or a file beside the binary.** Simpler path
  logic, but a `~/.editrc` clutters home and is odd on Windows, and a cwd/next-to-
  binary file is per-folder rather than per-user and easily lost — wrong for a
  system editor. The platform config dir is what users expect.
- **A strict parser that errors on bad input.** Rejected: a hand-edit typo or an
  older-version file would then block startup; lenient-and-total is friendlier and
  keeps forward compatibility free.
