# ADR 0026 — Adopt `rvision`'s `HelpWindow` and `resource` module, retiring their `edit`-side equivalents

- **Status:** Accepted
- **Date:** 2026-07-06

## Context

`edit` was pinned to `rvision` v0.1.0 for most of its life and hand-rolled two
things the framework didn't have yet: a modal help viewer (`help::HelpViewer`,
ADR 0013's "topic-list + scrollable-page viewer") and per-OS settings-path
resolution (`settings::config_path`, ADR 0025). The dependency is now bumped
to the published `rvision = "^2.0.1"`, which has since grown both natively:
`rvision::widgets::HelpWindow` (its own ADR 0016) and `rvision::resource`
(its own ADR 0024, which explicitly generalized `edit`'s own path-resolution
code once themes/help needed the same thing).

This is a narrow, low-risk pass, not the full "migrate `edit` onto the
framework" effort. A prior capability audit found most remaining hand-rolled
`edit` machinery (the global accelerator table, `StatusItem` unification, MDI
drag/resize/z-order, a `status_text`/`StatusPanel` line/column indicator) is
gated behind one unresolved question: whether `edit` adopts `rvision`'s
`Desktop`/`Window` at all, which requires solving the "reach a concrete
`Document` behind a `Window`'s `Box<dyn View>`" problem `rvision`'s own ADR
0016 explicitly declined to solve generically. The roadmap's Backlog already
flags that convergence as needing its own dedicated design session before any
code — this ADR does not touch it.

One candidate from the same audit — routing `edit`'s scrollbar-thumb dragging
through `rvision`'s `Context::request_mouse_capture` (ADR 0027) — turned out
on closer inspection not to fit: that capability exists for a widget buried
inside another view's interior that a `Desktop`-level dispatcher can't see
directly (e.g. a `ScrollBar` inside `FileDialog`/`HelpPane`). In `edit`,
`app::EditorApp::handle_mouse` already *is* the top-level dispatcher and
already directly hit-tests its own scrollbar; its `Drag` enum already unifies
Move/Resize/ScrollThumb in the same shape `Desktop` uses internally. Adopting
the `Context` protocol there would add indirection with nothing to gain, so
it's excluded from this pass.

## Decision

**Help viewer.** `help::HelpViewer` (the hand-rolled `ListBox`+`HelpPane`
composite) is deleted. `app::open_help` now builds `rvision::widgets::
HelpWindow` directly and runs it through `exec_view`, exactly like every
other editor modal: `HelpWindow::build`/`build_at` return a plain `Window`
(via `Window::new`, not `Window::dialog`, so it keeps the same
`Role::WindowFrame`/`WindowTitle` styling `HelpViewer` used), which needs the
same builder chain `dialogs::modal_window` already applies elsewhere —
`.centered().esc_cancels(true).also_ends_on(CM_CANCEL)` — since `Window::new`'s
defaults (`esc_cancels: false`, `ending: Vec::new()`) don't end an `exec_view`
loop on `Esc` by themselves. `HelpWindow` is documented as "meant to be opened
non-modally via `Desktop::open`" — that's a recommendation for its primary use
case, not a hard restriction; `exec_view` only needs a `&mut Window`.
`help::HELP_TEXT` and the content-validation tests (topic ids, link targets,
the clipboard topic) stay in `edit` — they validate `help.txt`'s content, not
the viewer.

**Settings path resolution.** `settings::config_path`'s per-OS branches
(`unix_config_path`/`macos_config_path`/`windows_config_path`) are deleted in
favour of `rvision::resource::user_resource_path(APP_DIR, FILE_NAME)`.
`edit`'s own `$EDIT_CONFIG_PATH` override check stays and runs first —
`rvision`'s ADR 0024 addendum explicitly declined to generalize an env-var
override into the framework, so `edit` keeps checking its own before falling
through. The on-disk `key = value` format (`parse`/`render`) is unaffected
(ADR 0025's format decision stands; only the *location* logic moved).

## Consequences

- `crates/edit/src/help.rs` shrinks to just the baked-in content and its
  content-validation tests; the viewer-behaviour tests move with the code
  they tested — they're now `rvision`'s own, covered by its test suite.
- `crates/edit/src/settings.rs` loses its three platform-specific resolvers
  and their unit tests; `explicit_override_wins_and_empty_is_ignored` (which
  tests `edit`'s own override, not path resolution) stays.
- One fewer place hand-rolling logic the framework now provides natively,
  with no behaviour change a user would notice (same env vars, same
  directories, same override).
- The bigger Desktop/Window convergence question (accelerators, MDI,
  drag/resize, status panel) remains open, unaffected by this ADR.

## Alternatives considered

- **Bundle this with the bigger Desktop/Window convergence.** Rejected —
  conflates a small, mechanical "use what the framework already ships"
  change with a genuinely open architectural question that needs its own
  design session, mirroring how the rvision dependency bump itself was kept
  separate from this follow-up work.
- **Also migrate scrollbar-thumb dragging onto `Context::request_mouse_capture`.**
  Rejected — see Context above; `edit`'s dispatcher already owns the same
  hit-testing directly, so the capability doesn't apply.

## Addendum (2026-07-06): the help viewer's `exec_view` decision was wrong

Running the migrated help window surfaced the problem directly: `Window::new`'s
fully-capable defaults draw a resize handle, zoom glyph, and close glyph —
none of which do anything under `exec_view`, which never wires up drag/resize
and only ends the loop on a command in `Window::ending` (just `CM_CANCEL`
here). A window that visually presents as movable/resizable/closable but
isn't is worse than one that plainly doesn't try. **ADR 0027 supersedes this
ADR's "Help viewer" decision specifically**: `edit` now hosts the help window
as a second, non-modal resident window in its own hand-rolled MDI, not via
`exec_view`. The "Settings path resolution" decision above is unaffected.
