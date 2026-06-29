# ADR 0020 — Drop shadows are a per-view protocol

- **Status:** Accepted
- **Date:** 2026-06-29

## Context

Phase 10 added the `Canvas::shadow(area, style)` primitive — the classic
TurboVision drop shadow (a two-column strip down the right, a one-row strip
below, each cell dimmed in place against `Role::Shadow`). But *who calls it* was
decided ad hoc, twice:

- `Application::exec_view` casts a shadow under every modal, unconditionally,
  resolving `Theme::default().style(Role::Shadow)` inline.
- The editor's bespoke `draw_window` (ADR 0018) casts a shadow under each MDI
  window itself, before drawing the window over it.

The reusable widgets got nothing. `Window::draw` drew its frame and interior but
no shadow, so the `chrome.rs` example — and anyone building on `Desktop` +
`Window` — saw flat, unshadowed windows. The framework owned the *primitive* but
not the *policy*, and the one consumer that did it "right" (the editor) wasn't
even using the widgets.

The structural reason a widget can't just shadow itself: a view draws through a
`Canvas` clipped to its own bounds (ADR 0015), but the shadow falls *outside*
those bounds, down the right and bottom edges. Only the owner, drawing on the
surface the view sits on, can paint it — and it must paint it *before* the child,
so the child (and any higher sibling) lands on top.

## Decision

Make casting a drop shadow a **property a view declares**, and painting it the
**owner's job**, via one new trait method:

```rust
trait View {
    fn drop_shadow(&self) -> Option<Style> { None }
}
```

`None` (the default) means flush — the view casts nothing. `Some(style)` means
the view floats and wants a shadow in `style`. Every container that composites
children — `Group::draw`, `Desktop::draw` — asks each child and, when it gets
`Some`, paints `canvas.shadow(child.bounds(), style)` on its own surface just
before drawing that child. `exec_view` does the same for the modal it centres.

The view supplies the *style*, not just a flag, because a view already resolves
its own colour roles at construction (ADR 0005); `Window`, `Dialog`, and
`FileDialog` resolve `Role::Shadow` then and hand it back here. The container
stays colour-blind — it never touches a `Theme`.

`Window` carries the per-widget switch the protocol implies: it casts by default
and `set_casts_shadow(false)` turns it off (e.g. a maximised window whose shadow
would only fall off-screen). Modal dialogs always float, so they always return
`Some`.

## Consequences

- Any `Window` on a `Desktop`, or any floating child of a `Group`, now gets a
  shadow for free — the `chrome.rs` example included — with no per-app code.
- `exec_view` no longer reaches for `Theme::default()`; it asks the modal. The
  inline theme lookup is gone from the application layer.
- Z-order falls out correctly: each child's shadow is painted immediately before
  the child, so a higher sibling's shadow lands on a lower one and every view
  sits over its own shadow — the same ordering the editor hand-codes.
- The editor's bespoke `draw_window` (ADR 0018) keeps its own `desk.shadow`
  call. It composites `Frame` + editor + scroll bars directly rather than via a
  `Window` widget, so there is no `View` object to carry `drop_shadow`. This is
  the documented escape hatch: the protocol serves widget-tree consumers; the
  bespoke driver opts out, consistently with why it exists at all.
- The look is uniform because every floating widget resolves the *same*
  `Role::Shadow`. The cost is that each stores its own resolved `Style` — a
  trivial redundancy, and the price of keeping containers theme-free.

## Alternatives considered

- **A `bool` flag + the container supplies the style.** Simpler signature, but
  the container would have to resolve `Role::Shadow` from a `Theme` — making
  `Group`/`Desktop` the first structural containers to know about theming, against
  ADR 0005's "views resolve roles." Rejected.
- **A `Window`-only `casts_shadow` field, no trait method.** Fixes the widget but
  not `Group`, and leaves `exec_view` and any future floating view each
  reinventing the policy. Rejected: the whole point was to stop doing it ad hoc.
- **A richer `Shadow { offset, style }` descriptor.** The offset is fixed in the
  `Canvas::shadow` primitive and no caller wants to vary it; `Option<Style>` is
  all the expressiveness there is demand for. Deferred until something needs it.
