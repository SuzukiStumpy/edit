# Module spec: `rvision::view`

- **Status:** Done
- **Phase:** 3 (View system)
- **Related ADRs:** 0003 (retained tree, parent-owns-children, commands up / broadcasts down), 0004 (three-phase dispatch, `EventResult`), 0015 (owner-relative coords + `Canvas`)

## Purpose

The retained-mode view tree. A [`View`] is the unit of the UI: it knows its
owner-relative `bounds`, draws itself through a [`Canvas`], and handles events. A
[`Group`] owns child views (`Vec<Box<dyn View>>`), draws them in z-order, and
implements the three-phase dispatch (positional → focused → broadcast) plus the
focus chain. [`Context`] is the handler's outbound channel: how a view posts a
command without holding a reference to anyone (ADR 0003).

It is **not** the widget library (buttons, windows — Phases 4/5) nor the app loop
(`app`). [`StaticText`] is the one concrete leaf here, plus a minimal focusable
test view to exercise dispatch.

## Public interface

```rust
pub trait View {
    fn bounds(&self) -> Rect;                 // in the owner's coordinates
    fn draw(&self, canvas: &mut Canvas);      // local (0,0) coords (ADR 0015)
    fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        let _ = (event, ctx);
        EventResult::Ignored                  // leaves that ignore everything
    }
    fn focusable(&self) -> bool { false }     // can this view hold focus?
    fn set_focused(&mut self, focused: bool) {}  // owner pushes focus (ADR 0017)
}

// A view runnable modally by app::exec_view (ADR 0017): adds size + ending commands.
pub trait Modal: View {
    fn size(&self) -> Size;
    fn ends_on(&self, command: Command) -> bool;
}

pub struct Context<'a> { /* posted queue + &CommandSet */ }
impl<'a> Context<'a> {
    pub fn new(commands: &'a CommandSet) -> Self;
    pub fn post(&mut self, command: Command);      // enabled-gated (ADR 0003)
    pub fn broadcast(&mut self, command: Command);
    pub fn is_enabled(&self, command: Command) -> bool;
    pub fn take_posted(&mut self) -> Vec<Event>;   // the app loop re-dispatches
}

pub struct StaticText { /* bounds, text, style */ }
impl StaticText { pub fn new(bounds: Rect, text: &str, style: Style) -> Self; }

pub struct Group { /* bounds, Vec<Box<dyn View>>, focused: Option<usize> */ }
impl Group {
    pub fn new(bounds: Rect, children: Vec<Box<dyn View>>) -> Self; // focuses first focusable
    pub fn focused(&self) -> Option<usize>;
}
```

## Behaviour & invariants

- **Coordinates.** `bounds` is owner-relative; `draw` is handed a `Canvas`
  already offset+clipped to those bounds, so a view draws at local `(0, 0)` and
  cannot paint outside its box (ADR 0015).
- **Z-order.** `Group` draws children in vector order: index 0 is bottom, last is
  top; later children overwrite earlier ones where they overlap. Focus order is
  the same vector order among `focusable` children.
- **Three-phase dispatch (per group, by event class — ADR 0004):**
  - *Positional* (`Mouse`): delivered to the **topmost** child whose `bounds`
    contain the pointer, with the position translated into that child's local
    coordinates. If it ignores, the group ignores (no redelivery to occluded
    siblings).
  - *Focused* (`Key`, `Command`): delivered to the focused child first. If the
    child ignores a `Tab`/`BackTab`, the group advances/retreats its own focus
    (wrapping among focusable children) and consumes it. Any other ignored event
    returns `Ignored` so it bubbles up the owner chain — that unwinding *is* the
    bubble (ADR 0003).
  - *Broadcast* (`Broadcast`, `Resize`, `Idle`): delivered to **all** children;
    the group returns `Ignored` (broadcasts don't stop).
- **Initial focus.** `Group::new` focuses the first `focusable` child, or `None`
  if there are none. A group with no focusable children ignores `Tab`.
- **Posting.** `Context::post` queues an `Event::Command` only if the command is
  enabled (a disabled control's command never fires); `broadcast` is not gated.
  The app loop later drains `take_posted` and re-injects them from the root.
- **Edge cases:** empty group (no children) ignores everything; a `Mouse` outside
  every child is ignored; `draw` of an off-screen child writes nothing (the
  `Canvas` clip handles it).

## Collaborators

- [`Canvas`] (draw), [`Buffer`] (via Canvas), `geometry` (`Rect`/`Point`).
- `event` (`Event`/`EventResult`/`KeyCode`), `command` (`Command`/`CommandSet`).
- Siblings never hold references to one another: a child posts a command via
  `Context`; the group routes events down; results unwind up (ADR 0003).

## Test plan (write these first)

- **Logic:** positional hit-testing picks the topmost containing child and
  translates the point; a click outside all children is ignored; initial focus is
  the first focusable child.
- **Interaction:** `Tab`/`BackTab` cycle focus among focusable children (skipping
  `StaticText`), wrapping at the ends; the focused child receives keys; a command
  a child posts appears in `Context` and (when the child ignores it) bubbles up.
- **Render (snapshot):** a group of two `StaticText`s plus an overlapping child
  draws in z-order at the right offsets.
- **Manual:** later, via the Phase 4 chrome demo.

## Open questions

- **Focus-aware drawing** (a focused button looking different): *resolved in Phase
  5* (ADR 0017). `draw` keeps its `&self`/`Canvas` signature; focus is a stored
  flag pushed by the owner via the new defaulted `View::set_focused`, which `Group`
  calls as focus moves (and forwards to its focused child). A richer draw context
  (theme/cursor too) is still open if a future need appears.
- **Integer view IDs** (ADR 0003) for targeted messages: not needed for bubbling
  or Tab traversal; add when a command must address a specific view.
- **Cross-group Tab boundary**: focus currently wraps within a group; handing off
  to the parent at the boundary is a Phase 4/5 refinement.
- **Theme threading** into `draw`: `StaticText` takes a concrete `Style` for now;
  resolving a `Role` against a `Theme` at draw time arrives with the chrome.
