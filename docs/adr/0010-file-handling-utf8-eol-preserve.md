# ADR 0010 — UTF-8 now, detect/preserve EOL, legacy deferred

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

We target Linux, Windows, and macOS, so file handling is where a cross-platform
editor quietly corrupts files. Three concerns: text encoding, line endings, and
tabs. Full multi-encoding support (Latin-1, Windows-1252, the authentically-DOS
CP437) is a data-table-and-UI affair, much like Unicode — a candidate to defer.

## Decision

- **Encoding:** read/write UTF-8 (preserve BOM and final-newline state). Non-UTF-8
  files handled gracefully (lossy load or open read-only) behind a **decode/encode
  seam**; legacy codepages added later behind that seam.
- **Line endings:** auto-detect LF/CRLF/CR on load and **preserve on save**;
  per-platform default for new files; EOL mode shown and switchable.
- **Tabs:** display width 8 (configurable); tabs stored literally by default.

## Consequences

- Robust on real-world files without upfront encoding scope.
- Never silently rewrites a file's line endings — a common, infuriating bug
  avoided by design.
- The decode/encode seam keeps multi-encoding a localised future addition.

## Alternatives considered

- **UTF-8 only, strict** — simplest, but real files hit it and frustrate.
- **Multi-encoding now** — most DOS-faithful (CP437!), but real extra scope
  (tables, conversion, UI) before core editing works.
