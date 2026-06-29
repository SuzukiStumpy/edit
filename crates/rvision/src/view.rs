//! The retained-mode view tree: the [`View`] trait, the owning [`Group`], the
//! handler [`Context`], and the [`StaticText`] leaf (ADR 0003, 0004).
//!
//! A [`View`] knows its owner-relative [`bounds`](View::bounds), draws itself
//! through a [`Canvas`] in local coordinates (ADR 0015), and handles events. A
//! [`Group`] owns its children (`Vec<Box<dyn View>>`, parent-owns-child), draws
//! them in z-order, and runs the three-phase dispatch — *positional* (mouse →
//! the view under the cursor), *focused* (keys/commands → the focus chain), and
//! *broadcast* (everyone) — plus Tab/Shift-Tab focus traversal.
//!
//! Views never reference one another. A view emits a command by posting it to the
//! [`Context`]; commands then bubble **up** the owner chain as the recursive
//! dispatch unwinds, and broadcasts travel **down** (ADR 0003).

use crate::canvas::Canvas;
use crate::color::Style;
use crate::command::{Command, CommandSet};
use crate::event::{Event, EventResult, KeyCode, MouseButton, MouseEvent, MouseKind};
use crate::geometry::{Point, Rect, Size};

/// One node of the UI tree.
///
/// The default [`handle_event`](View::handle_event) ignores everything and the
/// default [`focusable`](View::focusable) is `false`, so a passive leaf (like
/// [`StaticText`]) only implements [`bounds`](View::bounds) and
/// [`draw`](View::draw).
pub trait View {
    /// The view's rectangle in its **owner's** coordinate space.
    fn bounds(&self) -> Rect;

    /// Draws the view through `canvas`, which is already offset and clipped to
    /// the view's bounds: draw at local `(0, 0)` (ADR 0015).
    fn draw(&self, canvas: &mut Canvas);

    /// The drop shadow this view casts onto its **owner's** surface, or `None`
    /// (the default) if it casts none (ADR 0020).
    ///
    /// A shadow falls *outside* the view's own bounds — down its right and
    /// bottom edges — so a view cannot paint it through its own clipped
    /// [`draw`](Self::draw) canvas. Instead the owner reads this and paints
    /// `canvas.shadow(child.bounds(), style)` on its own surface *before* drawing
    /// the child on top, so each floating view sits over its own shadow and a
    /// higher sibling's shadow falls on a lower one. A floating widget — a
    /// [`Window`](crate::widgets::Window) or a modal
    /// [`Dialog`](crate::widgets::Dialog) — returns the
    /// [`Role::Shadow`](crate::theme::Role::Shadow) style it resolved at
    /// construction; flush views (controls, the desktop backdrop) keep `None`.
    fn drop_shadow(&self) -> Option<Style> {
        None
    }

    /// Handles one event, returning whether it was consumed (ADR 0004). A view
    /// emits its own commands by posting them to `ctx`; it never references
    /// another view.
    fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        let _ = (event, ctx);
        EventResult::Ignored
    }

    /// Whether this view can hold the keyboard focus.
    fn focusable(&self) -> bool {
        false
    }

    /// Notifies the view that it has gained (`true`) or lost (`false`) the
    /// keyboard focus, so a focusable view can draw itself focused (ADR 0017).
    ///
    /// The owning [`Group`] pushes this as focus moves; the default ignores it,
    /// so a view that does not draw differently when focused need not implement
    /// it. A `Group` forwards a `set_focused` it receives to its own focused
    /// child, so the signal composes through nested groups.
    fn set_focused(&mut self, focused: bool) {
        let _ = focused;
    }
}

/// A [`View`] that can be run modally by
/// [`Application::exec_view`](crate::app::Application::exec_view) (ADR 0017).
///
/// It adds the two things the modal loop needs beyond a plain view: the size to
/// centre the box at, and which commands end the loop (so the loop returns the
/// command that closed the modal). Both [`Dialog`](crate::widgets::Dialog) and the
/// file picker implement it.
pub trait Modal: View {
    /// The size [`exec_view`](crate::app::Application::exec_view) centres the modal
    /// at.
    fn size(&self) -> Size;

    /// Whether a posted `command` should close the modal loop (TurboVision's
    /// `endModal`). The loop returns the first such command.
    fn ends_on(&self, command: Command) -> bool;
}

/// A handler's outbound channel: how a view posts commands and queries command
/// state without holding a reference to anyone (ADR 0003).
///
/// One `Context` is threaded through a whole event's dispatch. Posted events
/// accumulate; the application loop drains them with [`take_posted`](Self::take_posted)
/// and re-injects them from the root.
pub struct Context<'a> {
    posted: Vec<Event>,
    commands: &'a CommandSet,
}

impl<'a> Context<'a> {
    /// Creates a context over the current command-enable state.
    pub fn new(commands: &'a CommandSet) -> Self {
        Self {
            posted: Vec::new(),
            commands,
        }
    }

    /// Posts a command to bubble up the owner chain — but only if it is enabled,
    /// so a disabled control's command never fires (ADR 0003).
    pub fn post(&mut self, command: Command) {
        if self.commands.is_enabled(command) {
            self.posted.push(Event::Command(command));
        }
    }

    /// Posts a broadcast to travel down to every view. Not gated by command
    /// state — a broadcast is a notification, not an action.
    pub fn broadcast(&mut self, command: Command) {
        self.posted.push(Event::Broadcast(command));
    }

    /// Whether `command` is currently enabled (e.g. so a control can grey itself).
    pub fn is_enabled(&self, command: Command) -> bool {
        self.commands.is_enabled(command)
    }

    /// The events posted so far during this dispatch.
    pub fn posted(&self) -> &[Event] {
        &self.posted
    }

    /// Takes the posted events, leaving the queue empty.
    pub fn take_posted(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.posted)
    }
}

/// A non-interactive line of text. The simplest concrete [`View`]: it draws its
/// string at its top-left and ignores every event.
pub struct StaticText {
    bounds: Rect,
    text: String,
    style: Style,
}

impl StaticText {
    /// Creates a static-text view occupying `bounds`, rendering `text` in `style`.
    pub fn new(bounds: Rect, text: &str, style: Style) -> Self {
        Self {
            bounds,
            text: text.to_string(),
            style,
        }
    }
}

impl View for StaticText {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.put_str(Point::new(0, 0), &self.text, self.style);
    }
}

/// A view that owns child views and routes events to them (ADR 0003, 0004).
///
/// Children are stored bottom-to-top: index 0 draws first (underneath), the last
/// child draws last (on top). Focus order is the same vector order among
/// [`focusable`](View::focusable) children.
pub struct Group {
    bounds: Rect,
    children: Vec<Box<dyn View>>,
    focused: Option<usize>,
}

impl Group {
    /// Creates a group occupying `bounds` that owns `children`. Focus starts on
    /// the first focusable child, or `None` if there is none; that child is told
    /// it holds the focus (ADR 0017).
    pub fn new(bounds: Rect, mut children: Vec<Box<dyn View>>) -> Self {
        let focused = children.iter().position(|child| child.focusable());
        if let Some(index) = focused {
            children[index].set_focused(true);
        }
        Self {
            bounds,
            children,
            focused,
        }
    }

    /// The index of the currently focused child, if any.
    pub fn focused(&self) -> Option<usize> {
        self.focused
    }

    /// Positional phase: deliver to the topmost child under the pointer, with the
    /// position translated into that child's local coordinates (ADR 0004). A
    /// left-press first moves focus to the clicked child if it can take it, so
    /// clicking a control focuses it the way TurboVision does.
    fn dispatch_positional(&mut self, mouse: MouseEvent, ctx: &mut Context) -> EventResult {
        for i in (0..self.children.len()).rev() {
            let bounds = self.children[i].bounds();
            if bounds.contains(mouse.pos) {
                if matches!(mouse.kind, MouseKind::Down(MouseButton::Left))
                    && self.children[i].focusable()
                {
                    self.set_focus(i);
                }
                let local = MouseEvent {
                    pos: mouse.pos.offset(-bounds.origin().x, -bounds.origin().y),
                    ..mouse
                };
                return self.children[i].handle_event(&Event::Mouse(local), ctx);
            }
        }
        EventResult::Ignored
    }

    /// Moves focus to child `index`, telling the old and new children (defaulted
    /// no-ops unless they draw themselves focused, ADR 0017).
    fn set_focus(&mut self, index: usize) {
        if self.focused != Some(index) {
            if let Some(old) = self.focused {
                self.children[old].set_focused(false);
            }
            self.children[index].set_focused(true);
            self.focused = Some(index);
        }
    }

    /// Focused phase: the focused child gets first crack; if it ignores a
    /// Tab/Shift-Tab, this group moves its own focus. Anything else ignored
    /// returns `Ignored` so it bubbles up the owner chain (ADR 0003).
    fn dispatch_focused(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        if let Some(index) = self.focused {
            if let Some(child) = self.children.get_mut(index) {
                if child.handle_event(event, ctx).is_consumed() {
                    return EventResult::Consumed;
                }
            }
        }
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Tab => return self.move_focus(true),
                KeyCode::BackTab => return self.move_focus(false),
                _ => {}
            }
        }
        EventResult::Ignored
    }

    /// Broadcast phase: deliver to every child; broadcasts don't stop, so the
    /// group reports `Ignored` (ADR 0004).
    fn dispatch_broadcast(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        for child in &mut self.children {
            child.handle_event(event, ctx);
        }
        EventResult::Ignored
    }

    /// Advances (or, if `!forward`, retreats) focus to the next focusable child,
    /// wrapping at the ends. Consumes the event, or ignores it if no child can
    /// take focus.
    fn move_focus(&mut self, forward: bool) -> EventResult {
        let focusable: Vec<usize> = (0..self.children.len())
            .filter(|&i| self.children[i].focusable())
            .collect();
        if focusable.is_empty() {
            return EventResult::Ignored;
        }
        let current = self
            .focused
            .and_then(|f| focusable.iter().position(|&i| i == f));
        let len = focusable.len();
        let next = match current {
            Some(p) if forward => (p + 1) % len,
            Some(p) => (p + len - 1) % len,
            None if forward => 0,
            None => len - 1,
        };
        self.set_focus(focusable[next]);
        EventResult::Consumed
    }
}

impl View for Group {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn draw(&self, canvas: &mut Canvas) {
        for child in &self.children {
            // A floating child casts its drop shadow on this group's surface
            // before it — and any higher sibling — is drawn on top (ADR 0020).
            if let Some(style) = child.drop_shadow() {
                canvas.shadow(child.bounds(), style);
            }
            let mut sub = canvas.child(child.bounds());
            child.draw(&mut sub);
        }
    }

    fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        match event {
            Event::Mouse(mouse) => self.dispatch_positional(*mouse, ctx),
            Event::Key(_) | Event::Command(_) => self.dispatch_focused(event, ctx),
            Event::Broadcast(_) | Event::Resize(_) | Event::Idle => {
                self.dispatch_broadcast(event, ctx)
            }
        }
    }

    fn focusable(&self) -> bool {
        // A group can take focus iff it has a focusable child to delegate to.
        self.focused.is_some()
    }

    fn set_focused(&mut self, focused: bool) {
        // Forward to the focused child so the signal composes through nesting
        // (ADR 0017): an outer group telling this one it (lost) gained focus
        // (un)focuses whichever of our children currently holds it.
        if let Some(index) = self.focused {
            self.children[index].set_focused(focused);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::cell::Cell;
    use crate::command::CM_OK;
    use crate::event::{KeyEvent, Modifiers, MouseButton, MouseKind};
    use crate::geometry::Size;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A shared event log: `(probe id, event)` in dispatch order. Lets a test
    /// inspect what a boxed, type-erased child saw.
    type Log = Rc<RefCell<Vec<(u16, Event)>>>;

    /// A focusable test view: records every event it handles into a shared log,
    /// fills its area with its id digit (so z-order is visible in snapshots), and
    /// optionally posts a command when a given key arrives.
    struct Probe {
        id: u16,
        bounds: Rect,
        focusable: bool,
        log: Log,
        post: Option<(KeyCode, Command)>,
    }

    impl Probe {
        fn new(id: u16, bounds: Rect, focusable: bool, log: &Log) -> Self {
            Self {
                id,
                bounds,
                focusable,
                log: Rc::clone(log),
                post: None,
            }
        }

        fn posting(mut self, key: KeyCode, command: Command) -> Self {
            self.post = Some((key, command));
            self
        }

        fn boxed(self) -> Box<dyn View> {
            Box::new(self)
        }
    }

    impl View for Probe {
        fn bounds(&self) -> Rect {
            self.bounds
        }

        fn draw(&self, canvas: &mut Canvas) {
            let digit = char::from_digit(self.id as u32 % 10, 10).unwrap_or('?');
            let area = canvas.bounds();
            canvas.fill(area, &Cell::from_char(digit, Style::new()));
        }

        fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
            self.log.borrow_mut().push((self.id, *event));
            if let (Event::Key(key), Some((code, command))) = (event, self.post) {
                if key.code == code {
                    ctx.post(command);
                    return EventResult::Consumed;
                }
            }
            EventResult::Ignored
        }

        fn focusable(&self) -> bool {
            self.focusable
        }
    }

    fn rect(x: i16, y: i16, w: i16, h: i16) -> Rect {
        Rect::from_origin_size(Point::new(x, y), Size::new(w, h))
    }

    fn mouse_down_at(x: i16, y: i16) -> Event {
        Event::Mouse(MouseEvent {
            kind: MouseKind::Down(MouseButton::Left),
            pos: Point::new(x, y),
            modifiers: Modifiers::NONE,
        })
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, Modifiers::NONE))
    }

    // --- StaticText (tracer bullet) ---

    #[test]
    fn static_text_draws_its_string_and_is_not_focusable() {
        let text = StaticText::new(rect(0, 0, 10, 1), "hello", Style::new());
        assert!(!text.focusable());

        let mut buf = Buffer::new(Size::new(10, 1));
        let mut canvas = Canvas::new(&mut buf);
        let mut sub = canvas.child(text.bounds());
        text.draw(&mut sub);
        assert_eq!(buf.to_text(), "hello     ");
    }

    // --- Positional dispatch ---

    #[test]
    fn mouse_goes_to_the_child_under_it_in_local_coords() {
        let log: Log = Log::default();
        let mut group = Group::new(
            rect(0, 0, 20, 10),
            vec![
                Probe::new(1, rect(1, 1, 5, 5), false, &log).boxed(),
                Probe::new(2, rect(10, 1, 5, 5), false, &log).boxed(),
            ],
        );
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);

        // A click inside child 2's box reaches child 2 with a translated point.
        group.handle_event(&mouse_down_at(11, 2), &mut ctx);
        let seen = log.borrow();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, 2);
        assert_eq!(seen[0].1, mouse_down_at(1, 1));
    }

    #[test]
    fn mouse_goes_to_the_topmost_of_overlapping_children() {
        let log: Log = Log::default();
        let mut group = Group::new(
            rect(0, 0, 20, 10),
            vec![
                Probe::new(1, rect(0, 0, 10, 10), false, &log).boxed(),
                Probe::new(2, rect(2, 2, 5, 5), false, &log).boxed(), // drawn last = on top
            ],
        );
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);

        group.handle_event(&mouse_down_at(3, 3), &mut ctx);
        let seen = log.borrow();
        assert_eq!(seen.len(), 1, "only the topmost child receives the click");
        assert_eq!(seen[0].0, 2);
        assert_eq!(seen[0].1, mouse_down_at(1, 1));
    }

    #[test]
    fn mouse_outside_every_child_is_ignored() {
        let log: Log = Log::default();
        let mut group = Group::new(
            rect(0, 0, 20, 10),
            vec![Probe::new(1, rect(1, 1, 3, 3), false, &log).boxed()],
        );
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);

        let result = group.handle_event(&mouse_down_at(15, 8), &mut ctx);
        assert_eq!(result, EventResult::Ignored);
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn a_left_press_focuses_the_clicked_focusable_child() {
        let log: Log = Log::default();
        let mut group = Group::new(
            rect(0, 0, 20, 10),
            vec![
                Probe::new(1, rect(1, 1, 5, 5), true, &log).boxed(),
                Probe::new(2, rect(10, 1, 5, 5), true, &log).boxed(),
            ],
        );
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);

        group.handle_event(&mouse_down_at(11, 2), &mut ctx); // inside child 2
        assert_eq!(group.focused(), Some(1));
        group.handle_event(&mouse_down_at(2, 2), &mut ctx); // inside child 1
        assert_eq!(group.focused(), Some(0));
    }

    #[test]
    fn a_press_on_a_non_focusable_child_leaves_focus_alone() {
        let log: Log = Log::default();
        let mut group = Group::new(
            rect(0, 0, 20, 10),
            vec![
                Probe::new(1, rect(1, 1, 5, 5), true, &log).boxed(),
                Probe::new(2, rect(10, 1, 5, 5), false, &log).boxed(),
            ],
        );
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        group.handle_event(&mouse_down_at(2, 2), &mut ctx); // focus the focusable child 1
        assert_eq!(group.focused(), Some(0));
        group.handle_event(&mouse_down_at(11, 2), &mut ctx); // click the non-focusable child 2
        assert_eq!(group.focused(), Some(0), "focus stays on child 1");
    }

    // --- Focus traversal ---

    #[test]
    fn initial_focus_is_the_first_focusable_child() {
        let log: Log = Log::default();
        let group = Group::new(
            rect(0, 0, 20, 10),
            vec![
                Box::new(StaticText::new(rect(0, 0, 5, 1), "x", Style::new())),
                Probe::new(1, rect(0, 1, 5, 1), true, &log).boxed(),
                Probe::new(2, rect(0, 2, 5, 1), true, &log).boxed(),
            ],
        );
        assert_eq!(group.focused(), Some(1));
    }

    #[test]
    fn tab_and_back_tab_cycle_focus_skipping_static_text() {
        let log: Log = Log::default();
        let mut group = Group::new(
            rect(0, 0, 20, 10),
            vec![
                Box::new(StaticText::new(rect(0, 0, 5, 1), "x", Style::new())),
                Probe::new(1, rect(0, 1, 5, 1), true, &log).boxed(),
                Probe::new(2, rect(0, 2, 5, 1), true, &log).boxed(),
            ],
        );
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);

        assert_eq!(group.focused(), Some(1));
        assert_eq!(
            group.handle_event(&key(KeyCode::Tab), &mut ctx),
            EventResult::Consumed
        );
        assert_eq!(group.focused(), Some(2));
        // Tab wraps past the end back to the first focusable child.
        group.handle_event(&key(KeyCode::Tab), &mut ctx);
        assert_eq!(group.focused(), Some(1));
        // Shift-Tab goes the other way, wrapping past the start.
        group.handle_event(&key(KeyCode::BackTab), &mut ctx);
        assert_eq!(group.focused(), Some(2));
    }

    #[test]
    fn keys_reach_only_the_focused_child() {
        let log: Log = Log::default();
        let mut group = Group::new(
            rect(0, 0, 20, 10),
            vec![
                Probe::new(1, rect(0, 0, 5, 1), true, &log).boxed(),
                Probe::new(2, rect(0, 1, 5, 1), true, &log).boxed(),
            ],
        );
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);

        // Focus is on child 1; a plain character key reaches only it.
        group.handle_event(&key(KeyCode::Char('a')), &mut ctx);
        let seen = log.borrow();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, 1);
        assert_eq!(seen[0].1, key(KeyCode::Char('a')));
    }

    #[test]
    fn group_with_no_focusable_children_ignores_tab() {
        let log: Log = Log::default();
        let mut group = Group::new(
            rect(0, 0, 20, 10),
            vec![Probe::new(1, rect(0, 0, 5, 1), false, &log).boxed()],
        );
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);

        assert_eq!(group.focused(), None);
        assert_eq!(
            group.handle_event(&key(KeyCode::Tab), &mut ctx),
            EventResult::Ignored
        );
    }

    #[test]
    fn empty_group_ignores_everything() {
        let mut group = Group::new(rect(0, 0, 20, 10), vec![]);
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);

        assert!(group.focused().is_none());
        assert_eq!(
            group.handle_event(&key(KeyCode::Char('a')), &mut ctx),
            EventResult::Ignored
        );
        assert_eq!(
            group.handle_event(&mouse_down_at(1, 1), &mut ctx),
            EventResult::Ignored
        );
    }

    // --- Focus notification (ADR 0017) ---

    /// A focusable leaf that records its current focus flag, set by its owner.
    struct FocusSpy {
        bounds: Rect,
        focused: Rc<RefCell<bool>>,
    }

    impl View for FocusSpy {
        fn bounds(&self) -> Rect {
            self.bounds
        }
        fn draw(&self, _canvas: &mut Canvas) {}
        fn focusable(&self) -> bool {
            true
        }
        fn set_focused(&mut self, focused: bool) {
            *self.focused.borrow_mut() = focused;
        }
    }

    #[test]
    fn group_tells_its_initial_focused_child_it_has_focus() {
        let a = Rc::new(RefCell::new(false));
        let b = Rc::new(RefCell::new(false));
        let _group = Group::new(
            rect(0, 0, 10, 3),
            vec![
                Box::new(StaticText::new(rect(0, 0, 5, 1), "x", Style::new())),
                Box::new(FocusSpy {
                    bounds: rect(0, 1, 5, 1),
                    focused: Rc::clone(&a),
                }),
                Box::new(FocusSpy {
                    bounds: rect(0, 2, 5, 1),
                    focused: Rc::clone(&b),
                }),
            ],
        );
        assert!(
            *a.borrow(),
            "the first focusable child is told it has focus"
        );
        assert!(!*b.borrow(), "a later child is not");
    }

    #[test]
    fn tab_moves_the_focus_flag_between_children() {
        let a = Rc::new(RefCell::new(false));
        let b = Rc::new(RefCell::new(false));
        let mut group = Group::new(
            rect(0, 0, 10, 3),
            vec![
                Box::new(FocusSpy {
                    bounds: rect(0, 0, 5, 1),
                    focused: Rc::clone(&a),
                }),
                Box::new(FocusSpy {
                    bounds: rect(0, 1, 5, 1),
                    focused: Rc::clone(&b),
                }),
            ],
        );
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        group.handle_event(&key(KeyCode::Tab), &mut ctx);
        assert!(!*a.borrow(), "the old focus child is told it lost focus");
        assert!(*b.borrow(), "the new focus child is told it gained focus");
    }

    #[test]
    fn a_group_forwards_focus_to_its_focused_child() {
        // An outer container telling a nested group it lost focus must reach the
        // grandchild that actually holds it (composition, ADR 0017).
        let leaf = Rc::new(RefCell::new(false));
        let mut group = Group::new(
            rect(0, 0, 10, 3),
            vec![Box::new(FocusSpy {
                bounds: rect(0, 0, 5, 1),
                focused: Rc::clone(&leaf),
            })],
        );
        assert!(*leaf.borrow());
        group.set_focused(false);
        assert!(!*leaf.borrow(), "the unfocus reached the grandchild");
        group.set_focused(true);
        assert!(*leaf.borrow());
    }

    // --- Commands: posting, gating, and bubbling ---

    #[test]
    fn focused_child_posts_a_command() {
        let log: Log = Log::default();
        let mut group = Group::new(
            rect(0, 0, 20, 10),
            vec![
                Probe::new(1, rect(0, 0, 5, 1), true, &log)
                    .posting(KeyCode::Enter, CM_OK)
                    .boxed(),
            ],
        );
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);

        let result = group.handle_event(&key(KeyCode::Enter), &mut ctx);
        assert_eq!(result, EventResult::Consumed);
        assert_eq!(ctx.posted(), &[Event::Command(CM_OK)]);
    }

    #[test]
    fn a_disabled_command_is_not_posted() {
        let log: Log = Log::default();
        let mut group = Group::new(
            rect(0, 0, 20, 10),
            vec![
                Probe::new(1, rect(0, 0, 5, 1), true, &log)
                    .posting(KeyCode::Enter, CM_OK)
                    .boxed(),
            ],
        );
        let mut cs = CommandSet::new();
        cs.disable(CM_OK);
        let mut ctx = Context::new(&cs);

        group.handle_event(&key(KeyCode::Enter), &mut ctx);
        assert!(ctx.posted().is_empty(), "a disabled command never fires");
    }

    #[test]
    fn a_command_routes_down_the_focus_chain_and_bubbles_back_up() {
        // Outer group -> inner group -> leaf probe. A Command travels down the
        // focus chain to the leaf; the leaf ignores it, and the Ignored unwinds
        // back up the owner chain — that unwinding is the bubble (ADR 0003).
        let log: Log = Log::default();
        let inner = Group::new(
            rect(2, 1, 10, 8),
            vec![Probe::new(7, rect(0, 0, 5, 1), true, &log).boxed()],
        );
        let mut outer = Group::new(rect(0, 0, 20, 10), vec![Box::new(inner)]);
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);

        let result = outer.handle_event(&Event::Command(CM_OK), &mut ctx);
        assert_eq!(
            result,
            EventResult::Ignored,
            "unhandled command bubbles out the top"
        );
        let seen = log.borrow();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0], (7, Event::Command(CM_OK)), "it reached the leaf");
    }

    // --- Broadcast & z-order draw ---

    #[test]
    fn broadcast_reaches_every_child() {
        let log: Log = Log::default();
        let mut group = Group::new(
            rect(0, 0, 20, 10),
            vec![
                Probe::new(1, rect(0, 0, 5, 1), false, &log).boxed(),
                Probe::new(2, rect(0, 1, 5, 1), false, &log).boxed(),
            ],
        );
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);

        group.handle_event(&Event::Broadcast(CM_OK), &mut ctx);
        let seen = log.borrow();
        let ids: Vec<u16> = seen.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, vec![1, 2], "both children saw the broadcast");
    }

    // --- Drop-shadow protocol (ADR 0020) ---

    #[test]
    fn a_plain_view_casts_no_shadow() {
        let text = StaticText::new(rect(0, 0, 5, 1), "x", Style::new());
        assert_eq!(text.drop_shadow(), None);
    }

    /// A view that fills itself with 'X' and casts a shadow in a given style.
    struct ShadowBox {
        bounds: Rect,
        shadow: Style,
    }

    impl View for ShadowBox {
        fn bounds(&self) -> Rect {
            self.bounds
        }
        fn draw(&self, canvas: &mut Canvas) {
            let area = canvas.bounds();
            canvas.fill(area, &Cell::from_char('X', Style::new()));
        }
        fn drop_shadow(&self) -> Option<Style> {
            Some(self.shadow)
        }
    }

    #[test]
    fn a_group_paints_a_childs_drop_shadow_outside_the_child() {
        let shadow = crate::theme::Theme::default().style(crate::theme::Role::Shadow);
        // A 5×3 child at (2, 2): its right strip starts at x = 7, its body at x = 2.
        let group = Group::new(
            rect(0, 0, 20, 10),
            vec![Box::new(ShadowBox {
                bounds: rect(2, 2, 5, 3),
                shadow,
            })],
        );
        let mut buf = Buffer::new(Size::new(20, 10));
        let mut canvas = Canvas::new(&mut buf);
        group.draw(&mut canvas);

        // The right-edge shadow cell is repainted in the shadow style…
        assert_eq!(buf.get(Point::new(7, 3)).unwrap().style(), shadow);
        // …the child's own body sits on top, undimmed…
        let body = buf.get(Point::new(3, 3)).unwrap();
        assert_eq!(body.grapheme().to_string(), "X");
        assert_eq!(body.style(), Style::new());
        // …and a cell clear of both is untouched.
        assert_eq!(buf.get(Point::new(0, 0)).unwrap().style(), Style::new());
    }

    #[test]
    fn snapshot_children_draw_in_z_order() {
        let log: Log = Log::default();
        let group = Group::new(
            rect(0, 0, 8, 4),
            vec![
                Probe::new(1, rect(0, 0, 8, 4), false, &log).boxed(), // fills '1'
                Probe::new(2, rect(2, 1, 4, 2), false, &log).boxed(), // '2' on top
            ],
        );
        let mut buf = Buffer::new(Size::new(8, 4));
        let mut canvas = Canvas::new(&mut buf);
        group.draw(&mut canvas);
        insta::assert_snapshot!(buf.to_text());
    }
}
