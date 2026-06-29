# Module spec: `edit::dialogs`

- **Status:** In progress
- **Phase:** 7 (editing features) — sub-phase 7c
- **Related ADRs:** 0017 (modal dialogs via `exec_view` + focus-aware controls),
  0018 (editor app drives the modals)

## Purpose

The editor's own modal dialogs — Go to Line, Find, Replace — built in the `edit`
crate (they are editor concepts) but composed from generic `rvision` controls
(`InputLine`, `CheckBox`, `Button`). Each owns its controls **concretely** (like
`FileDialog`) so the driver can read the typed value back after
[`exec_view`](rvision::app::Application::exec_view) returns `CM_OK`, with no
downcast and no view IDs (ADR 0003/0017). `rvision` stays editor-agnostic.

## Public interface

```rust
pub struct GoToLine { /* input, ok, cancel, focus */ }
impl GoToLine {
    pub fn new(theme: &Theme, current_line: usize) -> Self; // seeded with the 1-based caret line
    pub fn line(&self) -> Option<usize>;                    // parsed 1-based line, after CM_OK
}
impl View for GoToLine { /* draw, handle_event, focusable */ }
impl Modal for GoToLine { /* size, ends_on = CM_OK | CM_CANCEL */ }

pub struct FindDialog { /* query input, case/whole-word/backwards checkboxes, buttons */ }
impl FindDialog {
    pub fn new(theme: &Theme) -> Self;
    pub fn query(&self) -> Query;       // needle + case/whole-word, after CM_OK
    pub fn backward(&self) -> bool;     // the "Search backwards" option
}

// 7c.3:
// pub struct ReplaceDialog { /* + replacement input, Change-All */ }
```

## Behaviour & invariants

- **Self-contained focus.** `Tab`/`BackTab` cycle the controls; the focused
  control draws focused (ADR 0017). `Esc` posts `CM_CANCEL`; `Enter` accepts
  (`CM_OK`) unless focus is on Cancel.
- **Value read after the fact.** The dialog never mutates the document; the driver
  reads `line()` / `query()` / options once `exec_view` returns `CM_OK` and then
  calls the matching editor method.
- **Validation is the reader's job.** `GoToLine::line` returns `None` for empty or
  non-numeric input; the editor clamps an out-of-range line into the document.

## Collaborators

- `rvision::widgets` — `InputLine`, `CheckBox`, `Button` (owned controls);
  `View`/`Modal`/`Context` (the modal contract); `Theme` (colours).
- `edit::app` — the driver runs each dialog via `exec_view` over the `EditorApp`
  background and applies the result to the `EditorView`.
- `edit::editor` — `go_to_line`, and (7c.2+) the search/replace methods.

## Test plan (write these first)

- **Interaction (no terminal):** typing a number then `Enter` yields that
  `line()`; `Esc` cancels; non-numeric/empty input yields `None`; `Tab` cycles
  focus; `Enter` on Cancel cancels.
- **Render (snapshot):** the laid-out dialog box.
- **Editor:** `go_to_line` clamps to `1..=line_count`, moves the caret to column 0,
  drops the selection, and reveals the line.

## Open questions

- Find/Replace option set (case, whole word, direction, wrap) finalised in 7c.2.
