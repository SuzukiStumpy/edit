# Module spec: `edit::app`

- **Status:** Done (Phase 6); extended for MDI (Phase 8)
- **Phase:** 6 (editor, single document) — sub-phase 6c; 8 (MDI)
- **Related ADRs:** 0016 (Shell/menu overlay), 0017 (modal dialogs), 0018 (bespoke
  driver loop), 0010 (file handling), 0009 (MDI, phased)

> Note: `docs/specs/app.md` covers `rvision::app` (the framework loop/Shell). This
> spec is the *editor's* application root, `edit::app`.

## Purpose

The editor application: the chrome (menu bar, the framed editor windows, a
status line) plus the bespoke driver loop that wires menu/status commands to the
active document and runs the modal file dialogs (ADR 0018). It is the `edit`
binary's root; `rvision` knows nothing of it.

For MDI (ADR 0009) the app owns a `Vec<Document>` with an active index — each
`Document` bundles an `EditorView`, the file path, and its [`Encoding`]. They are
held **concretely**, not as `rvision::Window`/`Box<dyn View>`, so file/edit ops
reach the editor with no downcast (ADR 0018); this is why the editor does not
reuse `rvision::Desktop`. Each `Document` also carries a `normal` rectangle
(desktop-local); an app-level `zoomed` flag maximises the active window over the
desktop. A fresh app starts zoomed, so the single-window look matches Phase 6/7.

## Public interface

```rust
pub const CM_NEW / CM_OPEN / CM_SAVE / CM_SAVE_AS: Command;   // CM_QUIT is rvision's
pub const CM_CLOSE / CM_NEXT_WINDOW / CM_PREV_WINDOW: Command;        // Window menu (8a.2)
pub const CM_CASCADE / CM_TILE / CM_ZOOM: Command;                    // Window menu (8b)

struct Document { editor: EditorView, path: Option<PathBuf>, encoding: Encoding } // private
pub struct EditorApp { /* menu_bar, status_line, documents: Vec<Document>, active, ... */ }
impl EditorApp {
    pub fn new(size: Size, theme: &Theme) -> Self;
    pub fn relayout(&mut self, size: Size);
    pub fn dispatch(&mut self, event: &Event, commands: &CommandSet) -> Vec<Command>;
    pub fn active_editor(&self) -> &EditorView; pub fn active_editor_mut(&mut self) -> &mut _;
    // windows (terminal-free, unit-tested):
    pub fn window_count(&self) -> usize; pub fn active_index(&self) -> usize;
    pub fn new_window(&mut self, theme: &Theme);          // File ▸ New
    pub fn activate(&mut self, index: usize);             // Alt+1…9
    pub fn next_window(&mut self); pub fn prev_window(&mut self);  // F6 / Shift-F6
    pub fn remove_active_window(&mut self);               // Close (last → fresh Untitled)
    pub fn toggle_zoom(&mut self);                        // Zoom / F5
    pub fn cascade(&mut self); pub fn tile(&mut self);    // Cascade / Tile
    // terminal-free file ops on the active document (unit-tested):
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
- **MDI (ADR 0009, Phase 8a.2):** `documents` is never empty; `active` indexes the
  focused window. New and Open each open a **new** window (the current document
  keeps its own), so neither prompts. Window switching — Alt+1…9 (`activate`), F6
  (`next_window`) / Shift-F6 (`prev_window`) — is a pure state change handled
  inside `dispatch` (a window-key pass between the menu and the editor), so an open
  menu still swallows those keys. Close (Window menu or Alt-F3) posts `CM_CLOSE`
  so the driver can run the discard guard first; closing the last window resets it
  to a fresh Untitled rather than removing it.
- **Layout (Phase 8b):** `draw_canvas` paints the backdrop, then the windows
  bottom-to-top — inactive first, the active one last with the doubled frame — each
  with its own scroll bars. A zoomed active window covers the desktop, so only it is
  drawn. `sync_layout` resizes every editor's bounds to its window interior after
  any change to the active window, zoom, sizes, or terminal size, so viewport and
  scroll metrics stay correct. Cascade steps `normal` rects down-right from the
  top-left; Tile fills the desktop with a roughly square grid (last row stretched);
  Zoom (F5) toggles `zoomed`. Window drag/resize is Phase 9 (mouse).
- **Discard guard:** Close on a modified window, and Exit on *any* modified window
  (`confirm_discard_all` walks every window), prompt Yes/No/Cancel; Save with no
  path falls through to Save As; an I/O error shows a message box.
- **Clipboard (ADR 0019):** `EditorApp` owns the `String` clipboard; the editor
  posts `CM_CUT`/`CM_COPY`/`CM_PASTE` (keys or the Edit menu) and `handle_clipboard`
  acts on them — Copy reads `selected_text`, Cut `take_selection`, Paste
  `insert_text`. It needs no terminal, so the driver runs it before the file
  commands and it is unit-tested headlessly.
- The title shows the file name (or `Untitled`) with a `*` when modified. When
  several windows share a base name, `window_title` appends a ` (n)` instance
  indicator (numbered in window order) so duplicates — e.g. `Untitled (1)`,
  `Untitled (2)` — are distinguishable.

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
