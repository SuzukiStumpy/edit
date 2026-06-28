# ADR 0005 — Semantic colour roles over a truecolour-ready type

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

The authentic MS-DOS Edit look is the 16-colour CGA palette (4-bit fg + 4-bit
bg). TurboVision chose colours via nested palette strings that remap indices up
the owner chain — clever (dialogs auto-recolour) but the most confusing part of
the framework and hard to test. We also want to keep the door open to truecolour
later without a structural rewrite.

## Decision

Views request colours by **semantic role** (e.g. `WindowFrame`, `MenuSelected`,
`ButtonFocused`, `EditorText`, `Selection`) resolved against a central, swappable
**`Theme`**. The cell colour type is **truecolour-ready from day one** —
`Color { Default, Named(Color16), Rgb(u8,u8,u8) }` — but shipped themes use only
the 16 named CGA colours initially (stored as their canonical RGB so the look is
identical in either mode).

## Consequences

- Widgets never name concrete colours, so they're insulated from the colour
  representation entirely.
- Adding truecolour later is a *theme + backend* change (≈ a day), not a
  structural one: no `Cell`/buffer/widget churn. The "back pocket" is an unused
  enum variant plus a backend `match` arm.
- Easy to add dark/mono themes; trivial to unit-test role → style resolution.
- A few extra bytes per cell for the richer colour (negligible: a screen is a few
  KB).

## Alternatives considered

- **16-colour + nested palettes** — faithful, but the notoriously confusing TV
  subsystem and hard to test.
- **16-colour, strict type (no RGB)** — slightly simpler now; truecolour later
  would mean refactoring `Cell`, buffer, diff, and backend together.
- **Full truecolour now** — real extra scope (capability detection, downgrade
  fallback, richer theme format) before we've drawn a window.
