# ADR 0028 — Adopt `rvision::arrange` for window-arrangement geometry

- **Status:** Accepted
- **Date:** 2026-07-13

## Context

`edit` owns its documents and chrome concretely — a `Vec<Document>`, its own
drag/resize state machine, its own cascade/tile layout — instead of using
`rvision::widgets::Desktop`/`Window`, because `Window` wraps `Box<dyn View>`
and reaching a concrete `Document` behind one would force a downcast or
`Rc<RefCell>`, both rejected by rvision's ADR 0003. `rvision`'s own ADR 0016
confirmed this constraint is still real and didn't attempt to remove it; the
question of actually solving it — converging `edit`'s bespoke MDI with
`Desktop`/`Window` — remains open in `docs/roadmap.md`'s backlog, unaddressed
by this decision.

What changed is narrower but concrete: rvision's ADR 0033 (filed after
`edit`'s help-overlay work, ADR 0027, made the duplication measurable)
extracted the window-arrangement *geometry* — chrome hit-testing, move/resize
session math, bounds clamping, cascade/tile layout — into a new top-level
`rvision::arrange` module: plain functions over `Rect`/`Point`/`Size`, no
knowledge of `View`, `Window`, or any concrete document type. It was written
explicitly so `edit` could adopt it later without needing ADR 0003's
constraint solved first. `edit` was already depending on `rvision = "^2.0.1"`,
which resolves to the `2.1.0` release `arrange` shipped in — no version bump
needed.

Comparing `edit`'s own code (`crates/edit/src/app.rs`) against `rvision::arrange`
directly:

- `edit`'s `clamp_rect` is byte-for-byte identical to `arrange::clamp_rect`.
- `edit`'s `cascade_slot`/the loop inside `EditorApp::tile` are identical to
  `arrange::cascade_slot`/`arrange::tile`, module up to `edit`'s `MIN_WINDOW`
  (`Size::new(3, 3)`) needing to be passed as `arrange`'s caller-supplied
  `min_size` argument rather than being a hardcoded constant.
- `edit`'s local `ChromeHit` enum and `chrome_hit`/`chrome_hit_at` methods
  already call `rvision::widgets::Frame::close_span`/`zoom_span` internally —
  the exact calls `arrange::chrome_hit` makes — just always passing
  `has_help: false` (an affordance `edit` doesn't have) and never gating
  `moveable`/`resizable`/`closable` (`edit`'s windows are always all three;
  only `zoomable` varies between a `Document` and the help overlay). The
  swap is a drop-in replacement, not a behavioural change.
- `edit`'s `Drag`/`HelpDrag` `Move`/`Resize` variants store an
  *offset-from-the-grabbed-cell*, where `arrange::ArrangeSession` stores an
  *anchor-plus-starting-bounds*. rvision's ADR 0033 proved these are only
  algebraically equivalent because both hit-tests constrain the initial grab
  to exactly one cell (the title row, or the bottom-right corner) — `edit`'s
  own `chrome_hit` already guarantees this, so the representations are safe
  to swap, but this is a real representational change, not a rename.

## Decision

Adopt `rvision::arrange` for all four pieces, landing as one branch with
three commits ordered by risk, in one PR:

1. **Zero-risk identical swaps**: `clamp_rect` and `cascade_slot`/`tile`
   are deleted from `edit` and replaced with calls to
   `arrange::clamp_rect`/`arrange::cascade_slot`/`arrange::tile`, passing
   `MIN_WINDOW` as `min_size`.
2. **`chrome_hit` unification**: `edit`'s local `ChromeHit` enum and
   `chrome_hit`/`chrome_hit_at` methods are deleted; call sites move to
   `arrange::chrome_hit(bounds, pos, ChromeFlags { .. })`, constructing
   `ChromeFlags` with `moveable`/`resizable`/`closable` always `true`,
   `zoomable` set per caller (`true` for a `Document`, `false` for the help
   overlay, matching today), and **`has_help: false` in both cases** — see
   Scope below.
3. **`Drag`/`HelpDrag` rework**: both switch their `Move`/`Resize` payload
   from `{ dx, dy }` offsets to `arrange::ArrangeSession`, built by
   `arrange::start_session`/advanced by `arrange::continue_session`. The
   session math moves from screen coordinates (with the drag state manually
   subtracting the desktop origin at every step) to the same **desktop-local**
   space `Document::normal`/`HelpOverlay::window`'s bounds already use —
   `chrome_hit` itself keeps working in screen coordinates (mouse events
   arrive in screen space), but the press position is converted to
   desktop-local once, at the point a session starts, removing the
   `- desk.x`/`- desk.y` arithmetic `drag_to`/`drag_help_to` previously
   repeated on every step. `Drag::ScrollThumb` (editor-content scrolling, not
   window geometry) has no `rvision::arrange` equivalent and is untouched.

**Scope boundaries, chosen deliberately:**

- **No ownership convergence.** `EditorApp` keeps its concrete `Vec<Document>`
  and bespoke driver loop (ADR 0018) unchanged. `rvision::arrange` has no
  opinion on ownership — it operates on `Rect`/`Point`/`Size` only — so
  adopting it doesn't move `edit` any closer to (or further from) using
  `Desktop`/`Window` directly. That question — whether to solve ADR 0003's
  concrete-access constraint at all — is untouched and stays in
  `docs/roadmap.md`'s backlog.
- **`Drag` and `HelpDrag` stay independent call sites.** Both now hold an
  `ArrangeSession` for their `Move`/`Resize` cases, but `drag_to` and
  `drag_help_to` are not merged into one shared function, even though the
  underlying session-continuation math is now identical between them. ADR
  0027 already declined to generalize these two for a single utility window;
  matching data shapes doesn't change that a `Document` still carries
  `ScrollThumb` and the help overlay doesn't, so the two call sites still
  read cleanly apart.
- **No context-sensitive help glyph.** `arrange::ChromeFlags::has_help` and
  `ChromeHit::Help` exist to support rvision's window-scoped context help
  (rvision's ADR 0021), a real feature `edit` doesn't have. This migration
  passes `has_help: false` everywhere, preserving current behaviour exactly.
  Adding per-window context help is a separate, later decision if a need for
  it appears.

**A pre-existing ambiguity, fixed in passing:** `EditorApp::status_key_command`'s
doc comment already cited "ADR 0028" for rvision's global keyboard
accelerator table, written before `edit` had one of its own. Since `edit` now
has its own ADR 0028, that comment is corrected to say "rvision's ADR 0028"
explicitly, matching how every other cross-repo ADR reference in this
codebase is written.

## Consequences

- One classification/session/layout implementation instead of two —
  `edit`'s own copies are deleted outright, not kept as a fallback.
- `edit` is positioned to pick up any future `rvision::arrange` improvement
  (a bug fix, a new capability) for free, the same way rvision's own
  `Desktop`/`Window` already do.
- The existing test suite (`chrome_hit`/drag/cascade/tile unit tests, the
  help-overlay interaction tests and snapshots) is the regression safety net
  for this migration, the same bar rvision's own ADR 0033 used: every test
  passing unchanged is the proof the swap preserves behaviour. No new test
  files are added; only what a given commit's diff isn't already covered by
  gets a new assertion.
- `edit`'s drag-session code becomes simpler (no more manually-threaded
  desktop-origin subtraction on every step), a side effect of routing through
  desktop-local coordinates rather than a design goal in itself.
- The bigger windowing-convergence question — full ownership convergence, or
  consciously accepting the two-implementation split — remains exactly as
  open as before this ADR. This is dedup of the parts that were never the
  actual blocker, not progress on the blocker itself.

## Alternatives considered

- **Solve ADR 0003's concrete-access problem now (full ownership
  convergence).** Rejected for this pass — see Scope above. Two prior
  ADRs (rvision's 0016, rvision's 0033) each separately declined to bundle
  this with a smaller, available win; nothing about `rvision::arrange`
  changes the calculus on the harder problem.
- **Leave the duplication as-is, consciously closing the roadmap backlog
  item.** Rejected: `rvision::arrange` was built for exactly this adoption,
  is already available at the pinned dependency version, and the swap is
  low-risk — declining it would mean maintaining a hand-rolled copy of code
  now owned and tested upstream for no benefit.
- **Unify `drag_to`/`drag_help_to` into one shared session-continuation
  helper**, now that both hold the same `ArrangeSession` payload. Rejected:
  revisits ADR 0027's reasoning without new justification — the two
  functions still differ in their non-session variants (`ScrollThumb` vs.
  none), and two short, independently-readable functions cost less than the
  abstraction would save.
- **Adopt `has_help`/context-sensitive help glyphs while already touching
  `chrome_hit` call sites.** Rejected as scope creep on a geometry-dedup
  pass: it is a real feature with its own UX questions (which topic for
  which window, how it opens the help overlay) that deserves its own
  decision, not a side effect of a refactor (CLAUDE.md: don't design for
  hypothetical future need).
- **Three separate PRs, one per commit**, mirroring the project's own
  dependency-bump-then-refactor sequencing convention. Rejected here: unlike
  a breaking dependency bump, none of these three stages is independently
  load-bearing for unrelated work — they're one decision reviewed as one
  story, with separate commits already giving a bisectable/revertable unit
  if one stage needs to be undone alone.
