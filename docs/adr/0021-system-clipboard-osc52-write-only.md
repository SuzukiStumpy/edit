# ADR 0021 — System clipboard via OSC 52, write-only

- **Status:** Accepted
- **Date:** 2026-06-29
- **Phase:** 10 (polish & cross-platform)

## Context

ADR 0019 shipped an **internal** clipboard: a `String` owned by `EditorApp`,
touched only in `handle_clipboard` (Cut/Copy/Paste), deliberately **terminal-free**
so it unit-tests headlessly. It also recorded that the *system* clipboard (OSC 52)
was deferred to Phase 10. This is that follow-up.

OSC 52 is a terminal escape — `ESC ] 52 ; c ; <base64> BEL` — that sets the host
clipboard, so a copy in the editor lands in the OS clipboard and pastes into other
apps (and, crucially, works over SSH where there is no local clipboard API). Two
forces shape how far to take it:

- **It crosses the render seam (ADR 0002).** Writing the escape is terminal I/O,
  which lives behind the `Backend`. The framework never calls the terminal
  directly, and `handle_clipboard`'s whole virtue was needing no terminal. So the
  capability has to reach the `Backend` without dragging a terminal into the
  editor's pure clipboard logic.
- **Read-back is a different animal from write.** Setting the clipboard is a
  fire-and-forget write every modern terminal accepts. *Reading* it (the `?` query
  form) is asynchronous — the terminal answers as an input event you must parse
  against a timeout — and is disabled by default in most terminals (xterm,
  foot, kitty, VTE) because a background program reading your clipboard is an
  exfiltration risk; several prompt the user. It is unreliable to depend on.

## Decision

Add OSC 52 **write** support and stop there — Cut/Copy mirror the internal
clipboard to the host; **Paste keeps reading the internal buffer**.

- A new defaulted seam method `Backend::set_clipboard(&mut self, text) ->
  io::Result<()>`. The default is a no-op, so `TestBackend` and the scripted test
  terminals keep compiling; `CrosstermBackend` overrides it to write the escape.
- `osc52::set_clipboard(text)` builds the escape from a hand-rolled standard
  Base64 encoder (no crate — ADR 0001/0012). It is a pure function, so the byte
  format is pinned by unit tests (the RFC 4648 vectors) with no terminal.
- `Application::set_clipboard` forwards to the backend. The editor's driver calls
  it right after `handle_clipboard` succeeds **for Cut/Copy only**;
  `handle_clipboard` itself stays terminal-free and headlessly testable, exactly
  as ADR 0019 left it. The decision of *what* to push is the editor's (`clipboard()`);
  the *I/O* is the driver's — the same split ADR 0019 used for the commands.

Inbound text from other apps is handled separately, also without read-back, via
the terminal's bracketed-paste protocol. (This ADR first claimed it would "ride"
the terminal's paste as typed input with no work; that proved wrong — see
**ADR 0022**, which enables bracketed paste and adds an `Event::Paste`.)

## Consequences

- Copy/Cut now escape the editor to the real OS clipboard, including over SSH,
  with no new dependency and no change to the editor's clipboard model.
- The headless-testability ADR 0019 fought for is intact: `handle_clipboard` is
  unchanged and terminal-free; the new I/O is one driver line, covered by a test
  using a clipboard-recording test terminal, and the escape format is covered by
  pure `osc52` tests.
- Paste does not reflect text copied in another app *via OSC 52*; it reflects the
  internal buffer (plus whatever the terminal's own paste types in). For a
  TurboVision-style editor this matches user expectation and every comparable TUI
  editor's default.
- The escape hatch is named: read-back (`OSC 52 ; c ; ?` + parsing the reply event
  with a timeout) can be added later behind the same `Backend` seam — a new
  `Backend::request_clipboard` and an event — without disturbing the write path or
  this decision. Until a real need appears, we don't pay for its fragility.

## Alternatives considered

- **Full round-trip (Paste reads via OSC 52 query).** The "complete" option, but
  it depends on a terminal feature that is off-by-default and security-gated, is
  racy (async reply + timeout), and would push clipboard *reads* into the event
  loop and parser. High cost, unreliable payoff; deferred behind the seam above.
- **A separate `Clipboard` trait rather than a `Backend` method.** Cleaner in the
  abstract, but there is exactly one real terminal backend and the capability *is*
  terminal output; a defaulted method on `Backend` adds the ability with zero
  churn to existing impls. Revisit only if a non-terminal backend needs it.
- **Pull in a base64 crate.** Trivial to hand-roll (one function, RFC 4648), and
  the crate budget (ADR 0001) exists precisely to resist this reflex.
- **Clipboard I/O inside `handle_clipboard`.** Would re-introduce a terminal
  dependency into the one method ADR 0019 kept pure, breaking its headless test.
  Rejected — the driver already owns the I/O edge.
