# ADR 0022 — Paste-in via bracketed paste

- **Status:** Accepted
- **Date:** 2026-06-29
- **Phase:** 10 (polish & cross-platform)

## Context

ADR 0021 added OSC 52 *write* (Cut/Copy reach the host clipboard) and argued that
the *inbound* direction — pasting external text into the editor — would "ride the
terminal's own paste," i.e. the terminal would inject the clipboard as ordinary
typed input. Testing showed that is false in practice: the editor could copy out
but not paste in.

Two reasons:

- The real backend **enabled no paste mode and dropped the paste event.**
  `CrosstermBackend::new` never sent `EnableBracketedPaste`, and `map_event`
  mapped `crossterm::Event::Paste(_)` to `None`. On terminals that bracket pastes
  anyway (tmux, many modern emulators), the pasted text arrived as a `Paste`
  event we silently discarded.
- **Raw keystroke paste is the wrong mechanism even when it arrives.** Injected
  character-by-character, a paste runs through the editor's key handler, so a
  pasted `Ctrl`-anything, a tab, or text matching a binding can fire commands;
  newlines fight auto-behaviours; and it is a storm of events, not one edit.
  Bracketed paste exists precisely to deliver the blob as data, not commands.

OSC 52 *read-back* was already rejected (ADR 0021) as async, off-by-default, and
security-gated — that judgement stands. The fix is the terminal's paste-side
protocol, not a clipboard query.

## Decision

Model paste as a first-class event and let the terminal bracket it.

- A new event variant `Event::Paste(String)` carries the pasted text as one
  chunk. This makes `Event` `Clone`-but-not-`Copy` (the first heap-bearing
  variant); dispatch already passes events by reference, so the only fallout was a
  handful of `*event` copies in test probes becoming `.clone()`.
- `CrosstermBackend` sends `EnableBracketedPaste` on startup (and
  `DisableBracketedPaste` in the panic-safe restore), and `map_event` now returns
  `Some(Event::Paste(text))`.
- Paste routes like a **focused** event: `Group` (and the editor's
  `Desktop`/dispatch) deliver it to the focused view. So it works in the editor
  *and* in dialog input lines, not just one place.
  - `EditorView` inserts it at the caret via the existing `insert_text` — the same
    reversible edit a `Ctrl-V` paste makes (ADR 0011), so it replaces a selection
    and is undoable.
  - `InputLine` takes only the printable characters, flattening newlines, since a
    single-line field cannot hold them.

## Consequences

- Paste-in works from any app, cleanly: one undoable edit, no synthetic-keystroke
  hazards, multi-line preserved in the editor and flattened in single-line fields.
- Together with ADR 0021's write side, the clipboard is now bidirectional without
  ever querying the terminal: **out** via OSC 52, **in** via bracketed paste.
- `Event` lost `Copy`. The cost is cosmetic (a few `.clone()`s in tests); the
  event model now admits payload-bearing variants, which is the honest shape.
- A terminal that supports neither bracketed paste nor OSC 52 still degrades to
  raw-keystroke paste (the old behaviour) — not great, but no worse than before,
  and such terminals are rare.
- **The two pastes don't unify, and can't.** `Ctrl-V` / Edit→Paste paste the
  editor's internal clipboard; external text only enters via the terminal's own
  paste (`Ctrl+Shift+V`), because a terminal app cannot read the system clipboard
  or trigger a paste itself (OSC 52 read stays rejected, ADR 0021). To soften the
  surprise (least astonishment): a bracketed paste also *mirrors* into the
  internal clipboard, so the two converge after first contact and a later `Ctrl-V`
  repeats the external text; and invoking Paste with an empty internal clipboard
  shows a one-line hint pointing at `Ctrl+Shift+V` (rare once anything is copied).
  Documenting the convention in the help/About is left to the Help-system item.

## Alternatives considered

- **Interpret raw keystroke paste (no bracketed mode).** What ADR 0021 assumed.
  Fragile (bindings fire, control chars, event storm) and the very problem
  bracketed paste solves. Rejected.
- **Keep `Event: Copy` by boxing the text or stashing it out-of-band.** `Box`/`Rc`
  are not `Copy` either, and a side-channel for the payload reintroduces the
  shared-state the event model avoids. Dropping `Copy` is cleaner and nearly free.
- **OSC 52 read-back for paste.** Already rejected in ADR 0021 and unrelated to the
  actual gap — bracketed paste is the terminal's intended inbound path.
