# Module spec: `edit::app`

- **Status:** Done
- **Phase:** 6 (editor, single document) — sub-phase 6c
- **Related ADRs:** 0016 (Shell/menu overlay), 0017 (modal dialogs), 0018 (bespoke
  driver loop), 0010 (file handling)

> Note: `docs/specs/app.md` covers `rvision::app` (the framework loop/Shell). This
> spec is the *editor's* application root, `edit::app`.

## Purpose

The editor application: the chrome (menu bar, a single framed editor window, a
status line) plus the bespoke driver loop that wires menu/status commands to the
owned [`EditorView`] and runs the modal file dialogs (ADR 0018). It is the `edit`
binary's root; `rvision` knows nothing of it.

## Public interface

```rust
pub const CM_NEW / CM_OPEN / CM_SAVE / CM_SAVE_AS: Command;   // CM_QUIT is rvision's

pub struct EditorApp { /* menu_bar, status_line, editor, size, path, encoding, ... */ }
impl EditorApp {
    pub fn new(size: Size, theme: &Theme) -> Self;
    pub fn relayout(&mut self, size: Size);
    pub fn dispatch(&mut self, event: &Event, commands: &CommandSet) -> Vec<Command>;
    // terminal-free file ops (unit-tested):
    pub fn new_file(&mut self);
    pub fn open_file(&mut self, path) -> io::Result<bool>;   // bool = decoded lossily
    pub fn open_or_new(&mut self, path) -> io::Result<bool>; // `edit FILE` case
    pub fn save_to(&mut self, path) -> io::Result<()>;
    pub fn is_modified(&self) -> bool; pub fn path(&self) -> Option<&Path>;
    pub fn start_dir(&self) -> PathBuf;
}
impl Program for EditorApp { /* draw used as exec_view background */ }

pub fn run<T: Backend + EventSource>(app: Application<T>, ed: EditorApp, theme: &Theme)
    -> io::Result<()>;
```

## Behaviour & invariants

- **Why a bespoke loop (ADR 0018):** modal dialogs run via
  `Application::exec_view`, which owns the terminal and can't be reached from
  inside the view tree, and the generic `Root` has no hook to interleave one.
  `dispatch` returns the *posted* commands (which `Program::handle_event` can't),
  so the driver can run a `FileDialog`/`MessageBox` and call the file ops directly.
- **Concrete ownership:** `EditorApp` owns the `EditorView`, so Open/Save reach the
  document with no downcast and no shared `Rc<RefCell>`.
- **Three local key passes:** menu bar (pre-process / modal while open) → editor
  (focused) → status line (post-process global keys), mirroring `Shell` (ADR 0016).
- **Layout:** menu row on top, status row on bottom, the editor window filling the
  middle; the editor's interior is the desktop region inset by the one-cell border;
  resize relays out and re-clamps the editor's scroll.
- **Discard guard:** New/Open/Exit on a modified document prompt Yes/No/Cancel;
  Save with no path falls through to Save As; an I/O error shows a message box.
- **Clipboard (ADR 0019):** `EditorApp` owns the `String` clipboard; the editor
  posts `CM_CUT`/`CM_COPY`/`CM_PASTE` (keys or the Edit menu) and `handle_clipboard`
  acts on them — Copy reads `selected_text`, Cut `take_selection`, Paste
  `insert_text`. It needs no terminal, so the driver runs it before the file
  commands and it is unit-tested headlessly.
- The title shows the file name (or `Untitled`) with a `*` when modified.

## Collaborators

- `crate::editor::EditorView` (owned), `crate::file` (load/save).
- `rvision`: `MenuBar`/`StatusLine`/`Frame` (chrome), `FileDialog`/`MessageBox` +
  `Application::exec_view` (modals), `Application`/`Backend`/`EventSource` (loop).

## Test plan (done)

- **Interaction (no terminal):** a printable key reaches the editor and marks it
  modified; Alt-X posts `CM_QUIT` via the status line; an open menu swallows
  typing; a menu item posts its command; layout puts the editor inside the frame.
- **File ops (temp files):** `new_file` clears doc + path; open-then-edit-then-save
  preserves CRLF byte-for-byte and clears the dirty flag.
- **Manual:** `cargo run -p edit [FILE]` — open/edit/save, Save As, the
  save-changes prompt, resize, and an always-restored terminal.

## Open questions

- Find/Replace, Go-to-line, and undo/redo are Phase 7b–c; the menus will grow
  then (the clipboard landed in 7a). A reusable "modal-capable root" could be
  lifted into `rvision` if a second application needs one (ADR 0018).
