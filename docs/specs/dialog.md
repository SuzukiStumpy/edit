# Module spec: `rvision::widgets::Dialog` + `app::exec_view` + `MessageBox`

- **Status:** Done (the file picker landed too; `exec_view` runs any `view::Modal`)
- **Phase:** 5 (Dialogs & controls)
- **Related ADRs:** 0017 (modal loop + focus-aware drawing), 0003/0004 (tree + dispatch), 0015 (`Canvas`), 0005 (roles), 0016 (`Shell`/`Application` it runs beside)

## Purpose

The modal piece of Phase 5: a `Dialog` (a bordered, titled box of
[controls](controls.md)) and the `Application::exec_view` loop that runs one
modally on top of the current screen and returns the command that closed it.
`MessageBox` is the canned info/confirm dialog built on top.

It is **not** a window (it is not on the desktop, it is not in the view tree) and
not a control — it *owns* controls.

## Public interface

```rust
// widgets::Dialog
pub struct Dialog { size, title, style, controls: Group, ending: Vec<Command>, default_cmd: Option<Command> }
impl Dialog {
    pub fn new(size: Size, title: &str, theme: &Theme, controls: Vec<Box<dyn View>>) -> Self;
    pub fn with_default(self, command: Command) -> Self;  // Enter activates this command
    pub fn also_ends_on(self, command: Command) -> Self;  // beyond CM_OK / CM_CANCEL
    pub fn ends_on(&self, command: Command) -> bool;
    pub fn size(&self) -> Size;
}
impl View for Dialog { /* draws grey box + title + controls; routes keys/mouse */ }

// app::Application::exec_view
impl<T: Backend + EventSource> Application<T> {
    pub fn exec_view(&mut self, background: &mut dyn Program, dialog: &mut dyn Modal) -> io::Result<Command>;
}

// widgets::MessageBox — builds a Dialog
pub struct MessageBox;
impl MessageBox {
    pub fn ok(title: &str, message: &str, theme: &Theme) -> Dialog;          // [ OK ]
    pub fn ok_cancel(title: &str, message: &str, theme: &Theme) -> Dialog;   // [ OK ] [ Cancel ]
    pub fn yes_no(title: &str, message: &str, theme: &Theme) -> Dialog;      // [ Yes ] [ No ]
}
```

## Behaviour & invariants

- **Dialog draw.** Fills its whole canvas with `DialogBackground`, strokes a
  single-line box, centres ` title ` on the top border, then draws its controls
  through a `child()` sub-canvas inset one cell on every side. Controls are a
  `Group`, so the focused control draws focused (ADR 0017) and Tab cycles them.
- **Dialog keys.** `Esc` posts `CM_CANCEL` (consumed). Otherwise the focused
  control gets first crack (a focused `Button` posts its command, Tab moves
  focus, an input line types). If the control declines and the key is `Enter`,
  the dialog posts its `default_cmd` (the default button). Mouse is translated
  into the interior and routed positionally (behaviour mostly Phase 9).
- **Ending commands.** `ends_on` is `true` for `CM_OK`/`CM_CANCEL` plus any added
  via `also_ends_on`. `exec_view` returns the first posted ending command.
- **exec_view loop (ADR 0017).** Each turn: fresh frame at `terminal.size()`,
  `background.draw` (no events to the background), centre the dialog, paint its
  `drop_shadow()` on the background then draw the dialog on top (ADR 0020 — any
  `Modal`, hence `&mut dyn Modal`), present, poll one event, dispatch to the
  dialog under a fresh
  (all-enabled) `CommandSet`. Drain posted events as `Root` does: a posted
  *ending* command returns from the loop; any other posted command/broadcast is
  re-dispatched into the dialog. The dialog never joins the application tree; it
  is dropped when the loop returns.
- **MessageBox.** Centres the message on the first interior row and a centred row
  of buttons below; the first button is the default and every button command is
  registered as ending (so any button closes it). `ok` → `[CM_OK]`; `ok_cancel`
  → `CM_OK`/`CM_CANCEL`; `yes_no` → `CM_YES`/`CM_NO` (also-ends-on both).

## Collaborators

- `view::{View, Group, Context}`, `command::{Command, CommandSet, CM_OK, CM_CANCEL, CM_YES, CM_NO}`.
- `widgets::{Button, Label}` (controls it lays out), `theme::{Role, Theme}`.
- `app::{Application, Program}` + `backend::{Backend, EventSource}` for `exec_view`.
- `Canvas`/`Buffer`, `geometry`. Controls post via `Context`; never direct refs.

## Test plan (write these first)

- **Logic:** `ends_on` covers CM_OK/CM_CANCEL and added commands; `MessageBox`
  buttons and default; dialog size fits the message.
- **Render (snapshot):** a centred MessageBox over a background screen.
- **Interaction:** Esc → CM_CANCEL; Enter on the default button → its command;
  Tab moves focus between two buttons; a focused button's Space posts.
- **End-to-end (scripted terminal):** `exec_view` returns CM_OK when the script
  presses Enter on the OK button, CM_CANCEL on Esc; the background is drawn behind.
- **Manual:** the `dialogs` example.

## Open questions

- **Per-dialog command gating.** `exec_view` runs under an all-enabled
  `CommandSet`; greying a button until a field validates needs the dialog to own a
  `CommandSet` (or a validity hook). Deferred.
- **Hardware cursor** for a focused input line — drawn as a cell now (ADR 0017),
  real cursor when the editor needs it (Phase 6).
- **Menu reconciliation** (ADR 0016): the pull-down can become a modal view now
  that `exec_view` exists; left as a later cleanup.
