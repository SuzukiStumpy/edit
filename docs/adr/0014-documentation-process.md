# ADR 0014 — Full documentation process

- **Status:** Accepted
- **Date:** 2026-06-28

## Context

The project is explicitly to be "fully planned out… with documentation," and is a
learning exercise where the engineering *process* is itself part of the value.
The grilling that produced these ADRs is naturally a tree of discrete decisions,
which maps cleanly onto a documented record.

## Decision

Adopt a full documentation process:

- **ADRs** — one numbered record per significant decision (`docs/adr/`).
- **Roadmap** — phased plan; each phase names its modules, an interface sketch,
  and the tests to write first (`docs/roadmap.md`).
- **Per-module specs** — before building a module, fill in a short spec from
  `docs/module-spec-template.md` (purpose, interface, invariants, test plan).
- **Rustdoc** — doc comments on every public item; `#![warn(missing_docs)]` on.
- **CLAUDE.md** — conventions, build/test commands, architecture pointers for
  session continuity.

## Consequences

- Decisions are traceable; the rationale survives.
- Best engineering-process learning value; some writing overhead per module.
- Docs must be kept current — an ADR is superseded by a new ADR, not edited away;
  roadmap/spec entries update as plans shift.

## Alternatives considered

- **ADRs + roadmap + rustdoc + CLAUDE.md** (no per-module specs) — lighter, still
  traceable, but less rigorous design-before-code.
- **Rustdoc + README only** — minimal overhead, but discards the rationale trail
  the project explicitly wants.
