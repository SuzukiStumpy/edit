# ADR 0001 — Use crossterm at the OS/terminal boundary

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

This is a "build it ourselves" learning project, but cross-platform raw terminal
I/O is the one place where that ambition is most painful and least educational.
Linux/macOS need `termios` raw mode (via `ioctl`/`tcsetattr`), which Rust's std
does not expose; Windows needs the Console API (`SetConsoleMode`, virtual
terminal processing), a completely different model. None is reachable without
FFI into the OS. The real question is *how* we reach the OS, not whether.

## Decision

Use **crossterm** as the single dependency at the very bottom of the stack: raw
mode, the alternate screen, input events, and escape-sequence output. Everything
above crossterm — screen buffer, diffing, UI framework, editor — is hand-built.

## Consequences

- Learning time goes to framework and editor design, not `ioctl` quirks and the
  Windows Console surface.
- Cross-platform support (Linux/Windows/macOS) largely comes for free.
- crossterm is the only place FFI/`unsafe` lives; `rvision` itself sets
  `#![forbid(unsafe_code)]`.
- crossterm is confined to one module (`CrosstermBackend`, ADR 0002); the rest of
  the code depends on our own `Backend`/`EventSource` traits, so crossterm could
  be swapped without touching widgets.

## Alternatives considered

- **Hand-rolled FFI, zero deps** — declare termios/ioctl and Console functions
  ourselves. Maximum FFI learning, but a large yak-shave (especially Windows)
  for little payoff relative to the project's actual goals.
- **Minimal OS bindings (libc + windows-sys)** — build the whole terminal layer
  by hand over raw syscall bindings. Spirit-of-the-project, but still a big plumbing
  effort that isn't the interesting part.
