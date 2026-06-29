//! The editor application: the chrome (menu bar, framed editor window, status
//! line) plus the bespoke driver loop that wires menu/status commands to the
//! editor and runs the modal file dialogs (ADR 0018).
//!
//! Unlike a generic [`rvision::app::Shell`] + [`Root`](rvision::app::Root), this
//! root owns its [`EditorView`] **concretely**, so opening and saving reach the
//! document directly — no downcast, no shared interior mutability (ADR 0018). The
//! loop draws the screen, polls one event, dispatches it through three local
//! passes (menu → editor → status), and acts on whatever commands those passes
//! post: File ▸ Open/Save run an `exec_view` dialog and load/save the document.

use std::io;
use std::path::{Path, PathBuf};

use rvision::app::{Application, Program};
use rvision::backend::{Backend, EventSource};
use rvision::buffer::Buffer;
use rvision::canvas::Canvas;
use rvision::cell::Cell;
use rvision::color::Style;
use rvision::command::{CM_NO, CM_OK, CM_QUIT, CM_USER, CM_YES, Command, CommandSet};
use rvision::event::{Event, EventResult, KeyCode, KeyEvent, Modifiers};
use rvision::geometry::{Point, Rect, Size};
use rvision::theme::{Role, Theme};
use rvision::view::{Context, View};
use rvision::widgets::{
    FileDialog, Frame, Menu, MenuBar, MenuItem, MessageBox, ScrollBar, StatusItem, StatusLine,
};

use crate::dialogs::{FindDialog, GoToLine, ReplaceDialog};
use crate::editor::{
    CM_COPY, CM_CUT, CM_FIND, CM_FIND_NEXT, CM_GOTO, CM_PASTE, CM_REDO, CM_REPLACE, CM_UNDO,
    EditorView,
};
use crate::file::{self, Encoding};

/// File ▸ New.
pub const CM_NEW: Command = Command(CM_USER + 1);
/// File ▸ Open.
pub const CM_OPEN: Command = Command(CM_USER + 2);
/// File ▸ Save.
pub const CM_SAVE: Command = Command(CM_USER + 3);
/// File ▸ Save As.
pub const CM_SAVE_AS: Command = Command(CM_USER + 4);

/// Window ▸ Close — close the active window (also Alt-F3). Numbered well above the
/// editor's own command ids (`CM_USER + 10..18`) to keep the spaces disjoint.
pub const CM_CLOSE: Command = Command(CM_USER + 30);
/// Window ▸ Next — activate the next window (also F6).
pub const CM_NEXT_WINDOW: Command = Command(CM_USER + 31);
/// Window ▸ Previous — activate the previous window (also Shift-F6).
pub const CM_PREV_WINDOW: Command = Command(CM_USER + 32);
/// Window ▸ Cascade — stack the windows diagonally.
pub const CM_CASCADE: Command = Command(CM_USER + 33);
/// Window ▸ Tile — lay the windows out in a grid.
pub const CM_TILE: Command = Command(CM_USER + 34);
/// Window ▸ Zoom — maximise/restore the active window (also F5).
pub const CM_ZOOM: Command = Command(CM_USER + 35);

/// One open document: its editor view, the file's path, and the [`Encoding`] to
/// preserve on save (ADR 0010). `EditorApp` owns a `Vec<Document>` for MDI
/// (ADR 0009), concretely rather than as `Box<dyn View>` so file/edit operations
/// reach the editor with no downcast (ADR 0018).
struct Document {
    editor: EditorView,
    /// The open file's path, or `None` for an unsaved new document.
    path: Option<PathBuf>,
    /// The encoding to preserve on save (ADR 0010).
    encoding: Encoding,
    /// The window's un-zoomed rectangle in **desktop-local** coordinates (Phase
    /// 8b). The effective rect is this, unless the window is the zoomed active one,
    /// in which case it fills the desktop.
    normal: Rect,
}

impl Document {
    /// A new, empty, unsaved document with a focused editor. Its window rectangle
    /// (`normal`) and editor bounds are assigned by the owning [`EditorApp`].
    fn new(theme: &Theme) -> Self {
        let mut editor = EditorView::new(Rect::default(), theme);
        editor.set_focused(true);
        Self {
            editor,
            path: None,
            encoding: Encoding::new_file(),
            normal: Rect::default(),
        }
    }

    /// Whether the document has unsaved changes.
    fn is_modified(&self) -> bool {
        self.editor.is_modified()
    }

    /// The window title: the file name (or `Untitled`), with a `*` when modified.
    fn title(&self) -> String {
        let name = match &self.path {
            Some(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.to_string_lossy().into_owned()),
            None => "Untitled".to_string(),
        };
        if self.is_modified() {
            format!("{name} *")
        } else {
            name
        }
    }

    /// The directory a file dialog should start in: this file's folder, or the
    /// process working directory.
    fn start_dir(&self) -> PathBuf {
        self.path
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

/// The editor application root: chrome + the owned documents (one framed window
/// each) with an active index, plus the app-wide clipboard.
pub struct EditorApp {
    menu_bar: MenuBar,
    status_line: StatusLine,
    /// The open documents, drawn bottom-to-top; never empty.
    documents: Vec<Document>,
    /// Index into `documents` of the active (focused, topmost) window.
    active: usize,
    /// Whether the active window is maximised over the whole desktop (Phase 8b).
    /// A fresh app starts zoomed, matching the single-window look of Phase 8a.
    zoomed: bool,
    size: Size,
    /// The internal clipboard for Cut/Copy/Paste (ADR 0019; OSC 52 is Phase 10).
    clipboard: String,
    finished: bool,
    frame_style: Style,
    title_style: Style,
    backdrop: Cell,
}

/// The three chrome regions for a terminal of `size`.
struct Regions {
    menu: Rect,
    desktop: Rect,
    status: Rect,
}

fn regions(size: Size) -> Regions {
    let w = size.width.max(0);
    let h = size.height.max(0);
    Regions {
        menu: Rect::from_origin_size(Point::new(0, 0), Size::new(w, 1)),
        desktop: Rect::from_origin_size(Point::new(0, 1), Size::new(w, (h - 2).max(0))),
        status: Rect::from_origin_size(Point::new(0, (h - 1).max(0)), Size::new(w, 1)),
    }
}

/// `rect` inset by one cell on every side (the window border) — its interior.
fn inset1(rect: Rect) -> Rect {
    let Size { width, height } = rect.size();
    Rect::from_origin_size(
        rect.origin().offset(1, 1),
        Size::new((width - 2).max(0), (height - 2).max(0)),
    )
}

/// The smallest window that still has a usable interior (a 1-cell border plus at
/// least one interior cell).
const MIN_WINDOW: Size = Size::new(3, 3);

/// `rect` clamped to fit within a `bounds`-sized area at the origin: the size is
/// capped and the origin pulled back so the rectangle stays fully on the desktop.
fn clamp_rect(rect: Rect, bounds: Size) -> Rect {
    let w = rect.width().clamp(0, bounds.width.max(0));
    let h = rect.height().clamp(0, bounds.height.max(0));
    let x = rect.origin().x.clamp(0, (bounds.width - w).max(0));
    let y = rect.origin().y.clamp(0, (bounds.height - h).max(0));
    Rect::from_origin_size(Point::new(x, y), Size::new(w, h))
}

/// A cascade slot (desktop-local) for the `i`-th window on a `desktop`-sized area:
/// stepped down-right from the top-left and extending to the bottom-right corner,
/// so window 0 fills the desktop and later windows peek out behind it. The step
/// wraps so a long stack never marches off-screen.
fn cascade_slot(desktop: Size, i: usize) -> Rect {
    let step = (i % 8) as i16;
    let x = (step * 2).min((desktop.width - MIN_WINDOW.width).max(0));
    let y = step.min((desktop.height - MIN_WINDOW.height).max(0));
    clamp_rect(
        Rect::from_origin_size(
            Point::new(x, y),
            Size::new(desktop.width - x, desktop.height - y),
        ),
        desktop,
    )
}

impl EditorApp {
    /// Builds the editor application for a terminal of `size` with an empty,
    /// unsaved document.
    pub fn new(size: Size, theme: &Theme) -> Self {
        let menu_bar = MenuBar::new(
            regions(size).menu,
            vec![
                Menu::new(
                    "File",
                    vec![
                        MenuItem::new("New", CM_NEW),
                        MenuItem::new("Open...", CM_OPEN),
                        MenuItem::new("Save", CM_SAVE).with_shortcut("F2"),
                        MenuItem::new("Save As...", CM_SAVE_AS),
                        MenuItem::new("Exit", CM_QUIT).with_shortcut("Alt-X"),
                    ],
                ),
                Menu::new(
                    "Edit",
                    vec![
                        MenuItem::new("Undo", CM_UNDO).with_shortcut("Ctrl-Z"),
                        MenuItem::new("Redo", CM_REDO).with_shortcut("Ctrl-Y"),
                        MenuItem::new("Cut", CM_CUT).with_shortcut("Ctrl-X"),
                        MenuItem::new("Copy", CM_COPY).with_shortcut("Ctrl-C"),
                        MenuItem::new("Paste", CM_PASTE).with_shortcut("Ctrl-V"),
                    ],
                ),
                Menu::new(
                    "Search",
                    vec![
                        MenuItem::new("Find...", CM_FIND).with_shortcut("Ctrl-F"),
                        MenuItem::new("Find Next", CM_FIND_NEXT).with_shortcut("F3"),
                        MenuItem::new("Replace...", CM_REPLACE),
                        MenuItem::new("Go to Line...", CM_GOTO).with_shortcut("Ctrl-G"),
                    ],
                ),
                Menu::new(
                    "Window",
                    vec![
                        MenuItem::new("Next", CM_NEXT_WINDOW).with_shortcut("F6"),
                        MenuItem::new("Previous", CM_PREV_WINDOW).with_shortcut("Shift-F6"),
                        MenuItem::new("Zoom", CM_ZOOM).with_shortcut("F5"),
                        MenuItem::new("Cascade", CM_CASCADE),
                        MenuItem::new("Tile", CM_TILE),
                        MenuItem::new("Close", CM_CLOSE).with_shortcut("Alt-F3"),
                    ],
                ),
            ],
            theme,
        );
        let status_line = StatusLine::new(
            regions(size).status,
            vec![
                StatusItem::new(
                    "F2",
                    "Save",
                    KeyEvent::new(KeyCode::F(2), Modifiers::NONE),
                    CM_SAVE,
                ),
                // F3 is the editor's Find Next (consumed before the status line),
                // so Open lives on the File menu; no F3 accelerator here.
                StatusItem::new(
                    "Alt-X",
                    "Exit",
                    KeyEvent::new(KeyCode::Char('x'), Modifiers::ALT),
                    CM_QUIT,
                ),
            ],
            theme.style(Role::StatusBar),
            theme.style(Role::StatusKey),
        );
        let mut app = Self {
            menu_bar,
            status_line,
            documents: Vec::new(),
            active: 0,
            zoomed: true,
            size,
            clipboard: String::new(),
            finished: false,
            frame_style: theme.style(Role::WindowFrame),
            title_style: theme.style(Role::WindowTitle),
            backdrop: Cell::from_char('░', theme.style(Role::DesktopBackground)),
        };
        app.add_document(Document::new(theme));
        app
    }

    /// Repositions the chrome and the editors for a terminal of `size`, clamping
    /// each window's rectangle to fit the new desktop and re-syncing editor bounds.
    pub fn relayout(&mut self, size: Size) {
        self.size = size;
        let r = regions(size);
        self.menu_bar.set_bounds(r.menu);
        self.status_line.set_bounds(r.status);
        let ds = r.desktop.size();
        for doc in &mut self.documents {
            doc.normal = clamp_rect(doc.normal, ds);
        }
        self.sync_layout();
    }

    /// The desktop area (screen coordinates) the windows live in.
    fn desktop(&self) -> Rect {
        regions(self.size).desktop
    }

    /// Window `i`'s effective rectangle in **desktop-local** coordinates: its
    /// `normal` rect, unless it is the zoomed active window, which fills the
    /// desktop. Always clamped to the desktop.
    fn window_rect_local(&self, i: usize) -> Rect {
        let ds = self.desktop().size();
        if self.zoomed && i == self.active {
            Rect::from_origin_size(Point::new(0, 0), ds)
        } else {
            clamp_rect(self.documents[i].normal, ds)
        }
    }

    /// Sets every editor's bounds to its window's interior (screen coordinates), so
    /// viewport size and scroll metrics track the current layout. Call after any
    /// change to the active window, zoom, sizes, or the terminal size.
    fn sync_layout(&mut self) {
        let desktop = self.desktop();
        let ds = desktop.size();
        let origin = desktop.origin();
        for i in 0..self.documents.len() {
            let local = if self.zoomed && i == self.active {
                Rect::from_origin_size(Point::new(0, 0), ds)
            } else {
                clamp_rect(self.documents[i].normal, ds)
            };
            let screen =
                Rect::from_origin_size(local.origin().offset(origin.x, origin.y), local.size());
            self.documents[i].editor.set_bounds(inset1(screen));
        }
    }

    /// Whether a menu pull-down is open (for tests / the loop).
    pub fn menu_is_open(&self) -> bool {
        self.menu_bar.is_open()
    }

    /// The active document (always present — `documents` is never empty).
    fn doc(&self) -> &Document {
        &self.documents[self.active]
    }

    /// The active document, mutably.
    fn doc_mut(&mut self) -> &mut Document {
        &mut self.documents[self.active]
    }

    /// The active document's editor view.
    pub fn active_editor(&self) -> &EditorView {
        &self.doc().editor
    }

    /// The active document's editor view, mutably.
    pub fn active_editor_mut(&mut self) -> &mut EditorView {
        &mut self.doc_mut().editor
    }

    /// Whether the active document has unsaved changes.
    pub fn is_modified(&self) -> bool {
        self.doc().is_modified()
    }

    /// Whether the loop should stop.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// The active document's file path, if any.
    pub fn path(&self) -> Option<&Path> {
        self.doc().path.as_deref()
    }

    /// The active document's window title (file name or `Untitled`, `*` if dirty).
    fn title(&self) -> String {
        self.doc().title()
    }

    /// The directory a file dialog should start in: the active file's folder, or
    /// the process working directory.
    pub fn start_dir(&self) -> PathBuf {
        self.doc().start_dir()
    }

    // --- window management (terminal-free, unit-tested) ---

    /// The number of open windows (always ≥ 1).
    pub fn window_count(&self) -> usize {
        self.documents.len()
    }

    /// The active window's index.
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Pushes `doc` as a new window on top and makes it active, giving it a fresh
    /// cascade slot and re-syncing editor bounds.
    fn add_document(&mut self, mut doc: Document) {
        doc.normal = cascade_slot(self.desktop().size(), self.documents.len());
        self.documents.push(doc);
        self.active = self.documents.len() - 1;
        self.sync_layout();
    }

    /// Opens a fresh empty window and makes it active (File ▸ New).
    pub fn new_window(&mut self, theme: &Theme) {
        self.add_document(Document::new(theme));
    }

    /// Activates window `index` if it exists (Alt+1…9).
    pub fn activate(&mut self, index: usize) {
        if index < self.documents.len() {
            self.active = index;
            self.sync_layout();
        }
    }

    /// Activates the next window, wrapping (F6).
    pub fn next_window(&mut self) {
        let n = self.documents.len();
        self.activate((self.active + 1) % n);
    }

    /// Activates the previous window, wrapping (Shift-F6).
    pub fn prev_window(&mut self) {
        let n = self.documents.len();
        self.activate((self.active + n - 1) % n);
    }

    /// Removes the active window. The last window is never removed — it is reset to
    /// a fresh empty document instead, so there is always at least one. Otherwise
    /// the previous window in z-order becomes active.
    pub fn remove_active_window(&mut self) {
        if self.documents.len() == 1 {
            self.new_file();
            return;
        }
        self.documents.remove(self.active);
        if self.active >= self.documents.len() {
            self.active = self.documents.len() - 1;
        }
        self.sync_layout();
    }

    /// Maximises/restores the active window (Window ▸ Zoom, F5).
    pub fn toggle_zoom(&mut self) {
        self.zoomed = !self.zoomed;
        self.sync_layout();
    }

    /// Stacks the windows diagonally from the top-left, each reaching the
    /// bottom-right corner (Window ▸ Cascade). Turns zoom off.
    pub fn cascade(&mut self) {
        self.zoomed = false;
        let ds = self.desktop().size();
        for (i, doc) in self.documents.iter_mut().enumerate() {
            doc.normal = cascade_slot(ds, i);
        }
        self.sync_layout();
    }

    /// Lays the windows out in a grid that fills the desktop (Window ▸ Tile). Turns
    /// zoom off.
    pub fn tile(&mut self) {
        self.zoomed = false;
        let ds = self.desktop().size();
        let n = self.documents.len();
        // Roughly square grid; the last (possibly short) row stretches to fill.
        let cols = (1..=n).find(|c| c * c >= n).unwrap_or(1);
        let rows = n.div_ceil(cols);
        for i in 0..n {
            let row = i / cols;
            let col = i % cols;
            let cols_in_row = if row + 1 == rows {
                n - cols * row
            } else {
                cols
            };
            let cell_w = ds.width / cols_in_row as i16;
            let cell_h = ds.height / rows as i16;
            let x = cell_w * col as i16;
            let y = cell_h * row as i16;
            // The last column/row absorbs the integer-division remainder.
            let w = if col + 1 == cols_in_row {
                ds.width - x
            } else {
                cell_w
            };
            let h = if row + 1 == rows {
                ds.height - y
            } else {
                cell_h
            };
            self.documents[i].normal = Rect::from_origin_size(Point::new(x, y), Size::new(w, h));
        }
        self.sync_layout();
    }

    /// Handles a window-management key. Switching mutates `active` directly (no
    /// terminal needed); Close posts [`CM_CLOSE`] for the driver's discard guard.
    /// Returns `Consumed` when it was a window key.
    fn handle_window_key(&mut self, key: &KeyEvent, ctx: &mut Context) -> EventResult {
        match (key.code, key.modifiers) {
            (KeyCode::F(6), Modifiers::NONE) => self.next_window(),
            (KeyCode::F(6), Modifiers::SHIFT) => self.prev_window(),
            (KeyCode::F(5), Modifiers::NONE) => self.toggle_zoom(),
            // Alt-F3 closes; F3 alone stays the editor's Find Next.
            (KeyCode::F(3), Modifiers::ALT) => {
                ctx.post(CM_CLOSE);
                return EventResult::Consumed;
            }
            (KeyCode::Char(c), Modifiers::ALT) if c.is_ascii_digit() && c != '0' => {
                self.activate((c as u8 - b'1') as usize);
            }
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    // --- file operations (terminal-free, unit-tested) ---

    /// Resets the active document to empty and unsaved.
    pub fn new_file(&mut self) {
        let doc = self.doc_mut();
        doc.editor.set_text("");
        doc.path = None;
        doc.encoding = Encoding::new_file();
    }

    /// Loads `path` into the active document, adopting its encoding. Returns
    /// whether the bytes were decoded lossily (so the caller can warn).
    ///
    /// # Errors
    ///
    /// Propagates any I/O error from reading the file.
    pub fn open_file(&mut self, path: impl Into<PathBuf>) -> io::Result<bool> {
        let path = path.into();
        let loaded = file::load(&path)?;
        let doc = self.doc_mut();
        doc.editor.set_text(&loaded.text);
        doc.encoding = loaded.encoding;
        doc.path = Some(path);
        Ok(loaded.lossy)
    }

    /// Writes the active document to `path` (adopting it as its file) using the
    /// preserved encoding, then clears the dirty flag.
    ///
    /// # Errors
    ///
    /// Propagates any I/O error from writing the file.
    pub fn save_to(&mut self, path: impl Into<PathBuf>) -> io::Result<()> {
        let path = path.into();
        let doc = self.doc_mut();
        file::save(&path, &doc.editor.text(), &doc.encoding)?;
        doc.editor.mark_saved();
        doc.path = Some(path);
        Ok(())
    }

    /// Opens `path` into the active document if it exists, otherwise starts an
    /// empty document that will be created at `path` on the first save (the
    /// `edit FILE` command-line case).
    ///
    /// # Errors
    ///
    /// Propagates any I/O error from reading an existing file.
    pub fn open_or_new(&mut self, path: impl Into<PathBuf>) -> io::Result<bool> {
        let path = path.into();
        if path.exists() {
            self.open_file(path)
        } else {
            let doc = self.doc_mut();
            doc.editor.set_text("");
            doc.encoding = Encoding::new_file();
            doc.path = Some(path);
            Ok(false)
        }
    }

    // --- event dispatch ---

    /// Routes `event` through the three local passes (menu → editor → status) and
    /// returns the commands those passes posted, for the driver to act on
    /// (ADR 0018).
    pub fn dispatch(&mut self, event: &Event, commands: &CommandSet) -> Vec<Command> {
        let mut ctx = Context::new(commands);
        match event {
            Event::Key(key) => {
                // menu (pre-process) → window keys → active editor → status (post).
                let mut result = self.menu_bar.handle_event(event, &mut ctx);
                if result == EventResult::Ignored {
                    result = self.handle_window_key(key, &mut ctx);
                }
                if result == EventResult::Ignored {
                    result = self.doc_mut().editor.handle_event(event, &mut ctx);
                }
                if result == EventResult::Ignored {
                    self.status_line.handle_event(event, &mut ctx);
                }
            }
            Event::Idle | Event::Broadcast(_) => {
                self.menu_bar.handle_event(event, &mut ctx);
            }
            // Resize is handled by the driver (relayout); mouse is Phase 9.
            Event::Resize(_) | Event::Mouse(_) | Event::Command(_) => {}
        }
        ctx.take_posted()
            .into_iter()
            .filter_map(|e| match e {
                Event::Command(c) => Some(c),
                _ => None,
            })
            .collect()
    }

    /// Acts on a clipboard command (Cut/Copy/Paste), returning whether `command`
    /// was one. The app owns the clipboard so these need no terminal — the driver
    /// runs them before the dialog-bearing file commands (ADR 0019).
    pub fn handle_clipboard(&mut self, command: Command) -> bool {
        match command {
            CM_COPY => {
                if let Some(text) = self.active_editor().selected_text() {
                    self.clipboard = text;
                }
                true
            }
            CM_CUT => {
                if let Some(text) = self.active_editor_mut().take_selection() {
                    self.clipboard = text;
                }
                true
            }
            CM_PASTE => {
                if !self.clipboard.is_empty() {
                    let text = self.clipboard.clone();
                    self.active_editor_mut().insert_text(&text);
                }
                true
            }
            _ => false,
        }
    }

    /// The window draw order, bottom-to-top: inactive windows in z-order, then the
    /// active one on top. A zoomed active window covers the desktop, so only it is
    /// drawn.
    fn draw_order(&self) -> Vec<usize> {
        if self.zoomed {
            return vec![self.active];
        }
        let mut order: Vec<usize> = (0..self.documents.len())
            .filter(|&i| i != self.active)
            .collect();
        order.push(self.active);
        order
    }

    /// Draws the whole screen into `canvas`.
    fn draw_canvas(&self, canvas: &mut Canvas) {
        let r = regions(self.size);
        {
            let mut desk = canvas.child(r.desktop);
            let area = desk.bounds();
            desk.fill(area, &self.backdrop);
            for i in self.draw_order() {
                self.draw_window(&mut desk, i);
            }
        }
        self.status_line.draw(&mut canvas.child(r.status));
        self.menu_bar.draw(&mut canvas.child(r.menu));
        // The open pull-down draws last, over everything (ADR 0016).
        self.menu_bar.draw_overlay(canvas);
    }

    /// Draws window `i` (frame + editor + scroll bars) at its effective rectangle
    /// within the desktop canvas `desk`. The active window gets the doubled frame.
    fn draw_window(&self, desk: &mut Canvas, i: usize) {
        let local = self.window_rect_local(i);
        if local.is_empty() {
            return;
        }
        let doc = &self.documents[i];
        let mut win = desk.child(local);
        let area = win.bounds();
        Frame::new(&doc.title(), self.frame_style, self.title_style)
            .active(i == self.active)
            .draw(&mut win);
        let interior = inset1(area);
        if !interior.is_empty() {
            doc.editor.draw(&mut win.child(interior));
            self.draw_scrollbars(&mut win, area, &doc.editor);
        }
    }

    /// Draws the vertical and horizontal scroll bars over a window's right and
    /// bottom border, reflecting `editor`'s position in its document. `win` is the
    /// window's canvas and `area` its local bounds (`(0, 0)`-origin).
    fn draw_scrollbars(&self, win: &mut Canvas, area: Rect, editor: &EditorView) {
        let Size { width, height } = area.size();
        let m = editor.scroll_metrics();

        // Vertical bar on the right border, between the top and bottom corners.
        let vbar = Rect::from_origin_size(Point::new(width - 1, 1), Size::new(1, height - 2));
        if !vbar.is_empty() {
            let mut bar = ScrollBar::new(vbar, self.frame_style);
            bar.set_metrics(m.lines, m.viewport.height.max(0) as usize, m.top);
            bar.draw(&mut win.child(vbar));
        }

        // Horizontal bar on the bottom border, between the left and right corners.
        let hbar = Rect::from_origin_size(Point::new(1, height - 1), Size::new(width - 2, 1));
        if !hbar.is_empty() {
            let mut bar = ScrollBar::horizontal(hbar, self.frame_style);
            bar.set_metrics(
                m.content_width.max(0) as usize,
                m.viewport.width.max(0) as usize,
                m.left.max(0) as usize,
            );
            bar.draw(&mut win.child(hbar));
        }
    }
}

impl Program for EditorApp {
    fn draw(&mut self, frame: &mut Buffer) {
        let mut canvas = Canvas::new(frame);
        self.draw_canvas(&mut canvas);
    }

    /// Provided so `EditorApp` is a [`Program`] (the `exec_view` background only
    /// ever calls [`draw`](Self::draw)); the driver uses [`dispatch`](Self::dispatch)
    /// instead, since it needs the posted commands.
    fn handle_event(&mut self, event: &Event) -> EventResult {
        let _ = self.dispatch(event, &CommandSet::new());
        EventResult::Ignored
    }

    fn is_finished(&self) -> bool {
        self.finished
    }
}

/// Runs the editor over `app`'s terminal until the user exits (ADR 0018).
///
/// Each turn: relayout to the live terminal size, draw, present, then poll one
/// event, dispatch it, and act on every command it produced.
///
/// # Errors
///
/// Propagates any I/O error from presenting frames, polling events, or the modal
/// file dialogs.
pub fn run<T: Backend + EventSource>(
    mut app: Application<T>,
    mut ed: EditorApp,
    theme: &Theme,
) -> io::Result<()> {
    let commands = CommandSet::new();
    loop {
        let size = app.terminal().size();
        if size != ed.size {
            ed.relayout(size);
        }
        let mut frame = Buffer::new(size);
        ed.draw(&mut frame);
        app.terminal_mut().present(&frame)?;
        if ed.is_finished() {
            break;
        }

        let timeout = app.timeout();
        let event = app
            .terminal_mut()
            .poll_event(timeout)?
            .unwrap_or(Event::Idle);
        if let Event::Resize(new_size) = event {
            ed.relayout(new_size);
        }
        for command in ed.dispatch(&event, &commands) {
            handle_command(command, &mut app, &mut ed, theme)?;
        }
    }
    Ok(())
}

/// Acts on one command posted by the chrome.
fn handle_command<T: Backend + EventSource>(
    command: Command,
    app: &mut Application<T>,
    ed: &mut EditorApp,
    theme: &Theme,
) -> io::Result<()> {
    // Clipboard commands need no dialog/terminal; act on them first (ADR 0019).
    if ed.handle_clipboard(command) {
        return Ok(());
    }
    match command {
        // Undo/redo from the Edit menu route straight back to the editor (the keys
        // are handled in the editor itself).
        CM_UNDO => {
            ed.active_editor_mut().undo();
        }
        CM_REDO => {
            ed.active_editor_mut().redo();
        }
        CM_QUIT => {
            // Exit confirms *every* dirty window before quitting (ADR 0009).
            if confirm_discard_all(app, ed, theme)? {
                ed.finished = true;
            }
        }
        // New/Open each open a *new* window now — the current document keeps its
        // own, so there is nothing to discard (ADR 0009).
        CM_NEW => ed.new_window(theme),
        CM_OPEN => open(app, ed, theme)?,
        CM_SAVE => {
            save(app, ed, theme)?;
        }
        CM_SAVE_AS => {
            save_as(app, ed, theme)?;
        }
        CM_FIND => find(app, ed, theme)?,
        CM_FIND_NEXT => {
            ed.active_editor_mut().find_next(false);
        }
        CM_REPLACE => replace(app, ed, theme)?,
        CM_GOTO => go_to_line(app, ed, theme)?,
        CM_NEXT_WINDOW => ed.next_window(),
        CM_PREV_WINDOW => ed.prev_window(),
        CM_ZOOM => ed.toggle_zoom(),
        CM_CASCADE => ed.cascade(),
        CM_TILE => ed.tile(),
        CM_CLOSE => close(app, ed, theme)?,
        _ => {}
    }
    Ok(())
}

/// Runs the Replace dialog and replaces all matches, reporting the count.
fn replace<T: Backend + EventSource>(
    app: &mut Application<T>,
    ed: &mut EditorApp,
    theme: &Theme,
) -> io::Result<()> {
    let mut dialog = ReplaceDialog::new(theme);
    if app.exec_view(&mut *ed, &mut dialog)? != CM_OK {
        return Ok(());
    }
    let query = dialog.query();
    if query.needle.is_empty() {
        return Ok(());
    }
    let count = ed
        .active_editor_mut()
        .replace_all(&query, &dialog.replacement());
    let report = match count {
        0 => "Text not found.".to_string(),
        1 => "Replaced 1 occurrence.".to_string(),
        n => format!("Replaced {n} occurrences."),
    };
    message(app, ed, theme, "Replace", &report)
}

/// Runs the Find dialog and selects the first match (Find Next then repeats it).
fn find<T: Backend + EventSource>(
    app: &mut Application<T>,
    ed: &mut EditorApp,
    theme: &Theme,
) -> io::Result<()> {
    let mut dialog = FindDialog::new(theme);
    if app.exec_view(&mut *ed, &mut dialog)? == CM_OK {
        let query = dialog.query();
        if !query.needle.is_empty() {
            ed.active_editor_mut().find(query, dialog.backward());
        }
    }
    Ok(())
}

/// Runs the Go to Line dialog and moves the caret to the chosen line.
fn go_to_line<T: Backend + EventSource>(
    app: &mut Application<T>,
    ed: &mut EditorApp,
    theme: &Theme,
) -> io::Result<()> {
    let mut dialog = GoToLine::new(theme);
    if app.exec_view(&mut *ed, &mut dialog)? == CM_OK {
        if let Some(line) = dialog.line() {
            ed.active_editor_mut().go_to_line(line);
        }
    }
    Ok(())
}

/// Offers to save the active window's unsaved changes before a discarding action.
/// Returns whether it is OK to proceed (saved, or the user chose to discard);
/// `false` cancels.
fn confirm_discard<T: Backend + EventSource>(
    app: &mut Application<T>,
    ed: &mut EditorApp,
    theme: &Theme,
) -> io::Result<bool> {
    if !ed.is_modified() {
        return Ok(true);
    }
    let message = format!("Save changes to {}?", ed.title());
    let mut prompt = MessageBox::yes_no_cancel("Edit", &message, theme);
    match app.exec_view(&mut *ed, &mut prompt)? {
        CM_YES => save(app, ed, theme),
        CM_NO => Ok(true),
        _ => Ok(false),
    }
}

/// Confirms discarding *every* dirty window in turn (for Exit). Activates each so
/// the prompt and any Save target the right document; a single Cancel aborts and
/// leaves that window active. Returns whether it is OK to quit.
fn confirm_discard_all<T: Backend + EventSource>(
    app: &mut Application<T>,
    ed: &mut EditorApp,
    theme: &Theme,
) -> io::Result<bool> {
    for i in 0..ed.window_count() {
        ed.activate(i);
        if !confirm_discard(app, ed, theme)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Confirms discarding the active window, then closes it (Window ▸ Close). The
/// last window is reset to a fresh Untitled rather than removed.
fn close<T: Backend + EventSource>(
    app: &mut Application<T>,
    ed: &mut EditorApp,
    theme: &Theme,
) -> io::Result<()> {
    if confirm_discard(app, ed, theme)? {
        ed.remove_active_window();
    }
    Ok(())
}

/// Runs the Open dialog and loads the chosen file into a **new** window (ADR 0009),
/// leaving the current document open in its own.
fn open<T: Backend + EventSource>(
    app: &mut Application<T>,
    ed: &mut EditorApp,
    theme: &Theme,
) -> io::Result<()> {
    let mut dialog = FileDialog::open("Open File", ed.start_dir(), theme);
    if app.exec_view(&mut *ed, &mut dialog)? != CM_OK {
        return Ok(());
    }
    let path = dialog.path();
    ed.new_window(theme);
    match ed.open_file(path) {
        Ok(true) => message(
            app,
            ed,
            theme,
            "Open",
            "File was not valid UTF-8; loaded lossily.",
        ),
        Ok(false) => Ok(()),
        Err(err) => {
            // The load failed: drop the empty window we just opened for it.
            ed.remove_active_window();
            message(app, ed, theme, "Open failed", &err.to_string())
        }
    }
}

/// Saves to the current path, or runs Save As if there is none. Returns whether
/// the document was saved.
fn save<T: Backend + EventSource>(
    app: &mut Application<T>,
    ed: &mut EditorApp,
    theme: &Theme,
) -> io::Result<bool> {
    match ed.path().map(Path::to_path_buf) {
        Some(path) => write_to(app, ed, theme, path),
        None => save_as(app, ed, theme),
    }
}

/// Runs the Save As dialog and saves to the chosen path. Returns whether the
/// document was saved.
fn save_as<T: Backend + EventSource>(
    app: &mut Application<T>,
    ed: &mut EditorApp,
    theme: &Theme,
) -> io::Result<bool> {
    let mut dialog = FileDialog::save("Save As", ed.start_dir(), theme);
    if app.exec_view(&mut *ed, &mut dialog)? != CM_OK {
        return Ok(false);
    }
    let path = dialog.path();
    write_to(app, ed, theme, path)
}

/// Writes the document to `path`, reporting any I/O error in a message box.
/// Returns whether the write succeeded.
fn write_to<T: Backend + EventSource>(
    app: &mut Application<T>,
    ed: &mut EditorApp,
    theme: &Theme,
    path: PathBuf,
) -> io::Result<bool> {
    match ed.save_to(path) {
        Ok(()) => Ok(true),
        Err(err) => {
            message(app, ed, theme, "Save failed", &err.to_string())?;
            Ok(false)
        }
    }
}

/// Shows a one-button information box over the editor.
fn message<T: Backend + EventSource>(
    app: &mut Application<T>,
    ed: &mut EditorApp,
    theme: &Theme,
    title: &str,
    text: &str,
) -> io::Result<()> {
    let mut box_ = MessageBox::ok(title, text, theme);
    app.exec_view(&mut *ed, &mut box_)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> EditorApp {
        EditorApp::new(Size::new(40, 12), &Theme::default())
    }

    fn keydown(ed: &mut EditorApp, code: KeyCode, mods: Modifiers) -> Vec<Command> {
        ed.dispatch(&Event::Key(KeyEvent::new(code, mods)), &CommandSet::new())
    }

    #[test]
    fn typing_reaches_the_editor_and_marks_modified() {
        let mut ed = app();
        let posted = keydown(&mut ed, KeyCode::Char('h'), Modifiers::NONE);
        assert!(posted.is_empty(), "a printable key posts no command");
        assert!(ed.is_modified());
        assert_eq!(ed.active_editor().text(), "h");
    }

    #[test]
    fn alt_x_posts_quit_through_the_status_line() {
        let mut ed = app();
        let posted = keydown(&mut ed, KeyCode::Char('x'), Modifiers::ALT);
        assert_eq!(posted, vec![CM_QUIT]);
    }

    #[test]
    fn an_open_menu_swallows_typing() {
        let mut ed = app();
        keydown(&mut ed, KeyCode::Char('f'), Modifiers::ALT); // open File
        assert!(ed.menu_is_open());
        let posted = keydown(&mut ed, KeyCode::Char('a'), Modifiers::NONE);
        assert!(posted.is_empty());
        assert!(!ed.is_modified(), "the keystroke never reached the editor");
    }

    #[test]
    fn the_file_menu_selects_a_command() {
        let mut ed = app();
        keydown(&mut ed, KeyCode::Char('f'), Modifiers::ALT); // open File (New highlighted)
        let posted = keydown(&mut ed, KeyCode::Enter, Modifiers::NONE);
        assert_eq!(posted, vec![CM_NEW]);
    }

    // --- clipboard (ADR 0019) ---

    fn type_chars(ed: &mut EditorApp, s: &str) {
        for c in s.chars() {
            keydown(ed, KeyCode::Char(c), Modifiers::NONE);
        }
    }

    /// Drives the editor like the loop does: dispatch the key, then act on every
    /// command it posts (clipboard commands need no terminal).
    fn run_key(ed: &mut EditorApp, code: KeyCode, mods: Modifiers) {
        for command in keydown(ed, code, mods) {
            assert!(
                ed.handle_clipboard(command),
                "{command:?} is a clipboard cmd"
            );
        }
    }

    #[test]
    fn copy_then_paste_round_trips_through_the_clipboard() {
        let mut ed = app();
        type_chars(&mut ed, "abc");
        run_key(&mut ed, KeyCode::Home, Modifiers::CONTROL);
        run_key(&mut ed, KeyCode::End, Modifiers::SHIFT); // select "abc"
        run_key(&mut ed, KeyCode::Char('c'), Modifiers::CONTROL); // copy
        run_key(&mut ed, KeyCode::End, Modifiers::CONTROL); // caret to end, clear selection
        run_key(&mut ed, KeyCode::Char('v'), Modifiers::CONTROL); // paste
        assert_eq!(ed.active_editor().text(), "abcabc");
    }

    #[test]
    fn cut_removes_the_selection_and_paste_restores_it() {
        let mut ed = app();
        type_chars(&mut ed, "abc");
        run_key(&mut ed, KeyCode::Home, Modifiers::CONTROL);
        run_key(&mut ed, KeyCode::End, Modifiers::SHIFT); // select "abc"
        run_key(&mut ed, KeyCode::Char('x'), Modifiers::CONTROL); // cut
        assert_eq!(ed.active_editor().text(), "");
        run_key(&mut ed, KeyCode::Char('v'), Modifiers::CONTROL); // paste it back
        assert_eq!(ed.active_editor().text(), "abc");
    }

    #[test]
    fn paste_with_an_empty_clipboard_is_a_no_op() {
        let mut ed = app();
        type_chars(&mut ed, "x");
        assert!(ed.handle_clipboard(CM_PASTE));
        assert_eq!(ed.active_editor().text(), "x");
    }

    #[test]
    fn the_edit_menu_posts_its_commands() {
        let mut ed = app();
        keydown(&mut ed, KeyCode::Char('e'), Modifiers::ALT); // open Edit (Undo highlighted)
        assert!(ed.menu_is_open());
        let posted = keydown(&mut ed, KeyCode::Enter, Modifiers::NONE); // first item: Undo
        assert_eq!(posted, vec![CM_UNDO]);
        // Re-open and step down to Cut to confirm the clipboard items are wired too.
        keydown(&mut ed, KeyCode::Char('e'), Modifiers::ALT);
        keydown(&mut ed, KeyCode::Down, Modifiers::NONE); // Redo
        keydown(&mut ed, KeyCode::Down, Modifiers::NONE); // Cut
        let posted = keydown(&mut ed, KeyCode::Enter, Modifiers::NONE);
        assert_eq!(posted, vec![CM_CUT]);
    }

    #[test]
    fn ctrl_z_undoes_typing_through_the_app() {
        let mut ed = app();
        type_chars(&mut ed, "abc");
        assert_eq!(ed.active_editor().text(), "abc");
        keydown(&mut ed, KeyCode::Char('z'), Modifiers::CONTROL); // editor handles it
        assert_eq!(
            ed.active_editor().text(),
            "",
            "the typing run undoes as one unit"
        );
        assert!(!ed.is_modified(), "undone back to the empty saved state");
    }

    #[test]
    fn the_undo_menu_command_routes_to_the_editor() {
        let mut ed = app();
        type_chars(&mut ed, "z");
        assert!(ed.active_editor_mut().undo(), "a pending action to undo");
        assert!(ed.active_editor_mut().redo(), "and to redo");
        assert_eq!(ed.active_editor().text(), "z");
    }

    #[test]
    fn ctrl_f_posts_find_and_f3_repeats_in_the_editor() {
        let mut ed = app();
        type_chars(&mut ed, "ab ab");
        // Ctrl+F bubbles up CM_FIND for the driver to run the dialog.
        let posted = keydown(&mut ed, KeyCode::Char('f'), Modifiers::CONTROL);
        assert_eq!(posted, vec![CM_FIND]);
        // Seed a query directly, then F3 (Find Next) is handled inside the editor.
        ed.active_editor_mut()
            .find(crate::search::Query::new("ab"), false); // selects 0..2
        let posted = keydown(&mut ed, KeyCode::F(3), Modifiers::NONE);
        assert!(
            posted.is_empty(),
            "F3 is consumed by the editor, posts nothing"
        );
        assert_eq!(
            ed.active_editor().cursor(),
            crate::text::Position::new(0, 5)
        );
    }

    #[test]
    fn the_search_menu_lists_find_find_next_replace_and_go_to_line() {
        let mut ed = app();
        keydown(&mut ed, KeyCode::Char('s'), Modifiers::ALT); // open Search
        let find = keydown(&mut ed, KeyCode::Enter, Modifiers::NONE); // first item: Find...
        assert_eq!(find, vec![CM_FIND]);
        keydown(&mut ed, KeyCode::Char('s'), Modifiers::ALT);
        keydown(&mut ed, KeyCode::Down, Modifiers::NONE); // Find Next
        let next = keydown(&mut ed, KeyCode::Enter, Modifiers::NONE);
        assert_eq!(next, vec![CM_FIND_NEXT]);
        keydown(&mut ed, KeyCode::Char('s'), Modifiers::ALT);
        keydown(&mut ed, KeyCode::Down, Modifiers::NONE); // Find Next
        keydown(&mut ed, KeyCode::Down, Modifiers::NONE); // Replace...
        let replace = keydown(&mut ed, KeyCode::Enter, Modifiers::NONE);
        assert_eq!(replace, vec![CM_REPLACE]);
    }

    #[test]
    fn ctrl_g_and_the_search_menu_post_go_to_line() {
        let mut ed = app();
        // The key posts it straight from the editor.
        let posted = keydown(&mut ed, KeyCode::Char('g'), Modifiers::CONTROL);
        assert_eq!(posted, vec![CM_GOTO]);
        // And the Search menu's "Go to Line..." item (fourth) posts the same command.
        keydown(&mut ed, KeyCode::Char('s'), Modifiers::ALT); // open Search
        assert!(ed.menu_is_open());
        keydown(&mut ed, KeyCode::Down, Modifiers::NONE); // Find Next
        keydown(&mut ed, KeyCode::Down, Modifiers::NONE); // Replace...
        keydown(&mut ed, KeyCode::Down, Modifiers::NONE); // Go to Line...
        let posted = keydown(&mut ed, KeyCode::Enter, Modifiers::NONE);
        assert_eq!(posted, vec![CM_GOTO]);
    }

    #[test]
    fn scrollbars_draw_on_the_window_frame_and_track_the_position() {
        use rvision::buffer::Buffer;
        use rvision::canvas::Canvas;

        let mut ed = EditorApp::new(Size::new(20, 10), &Theme::default());
        // 20 lines, each wider than the interior (18): both axes overflow.
        let line = "abcdefghijklmnopqrstuvwxyz";
        ed.active_editor_mut().set_text(
            &std::iter::repeat(line)
                .take(20)
                .collect::<Vec<_>>()
                .join("\n"),
        );

        let render = |ed: &EditorApp| {
            let mut buf = Buffer::new(Size::new(20, 10));
            ed.draw_canvas(&mut Canvas::new(&mut buf));
            buf
        };
        let glyph = |buf: &Buffer, x: i16, y: i16| {
            buf.get(Point::new(x, y)).unwrap().grapheme().to_string()
        };

        let buf = render(&ed);
        // Vertical bar down the right column (x=19), horizontal along the bottom row.
        assert_eq!(glyph(&buf, 19, 2), "▲");
        assert_eq!(glyph(&buf, 19, 7), "▼");
        assert_eq!(glyph(&buf, 1, 8), "◄");
        assert_eq!(glyph(&buf, 18, 8), "►");
        // At the top-left both thumbs sit just past the leading arrow.
        assert_eq!(glyph(&buf, 19, 3), "█", "vertical thumb at top");
        assert_eq!(glyph(&buf, 2, 8), "█", "horizontal thumb at left");

        // Jump to the document end: the vertical thumb moves toward the bottom.
        keydown(&mut ed, KeyCode::End, Modifiers::CONTROL);
        let buf = render(&ed);
        assert_eq!(glyph(&buf, 19, 6), "█", "vertical thumb near the bottom");
    }

    #[test]
    fn layout_puts_the_editor_inside_the_framed_desktop() {
        let ed = EditorApp::new(Size::new(30, 10), &Theme::default());
        // Desktop region is rows 1..9 (8 tall); the editor sits one cell inside it.
        assert_eq!(
            ed.active_editor().bounds(),
            inset1(regions(Size::new(30, 10)).desktop)
        );
    }

    #[test]
    fn new_file_clears_the_document_and_path() {
        let mut ed = app();
        keydown(&mut ed, KeyCode::Char('z'), Modifiers::NONE);
        ed.documents[ed.active].path = Some(PathBuf::from("/tmp/whatever.txt"));
        ed.new_file();
        assert_eq!(ed.active_editor().text(), "");
        assert!(ed.path().is_none());
        assert!(!ed.is_modified());
    }

    #[test]
    fn open_then_save_round_trips_a_file_preserving_eol() {
        let path = std::env::temp_dir().join(format!("edit-app-test-{}.txt", std::process::id()));
        std::fs::write(&path, b"one\r\ntwo\r\n").unwrap();

        let mut ed = app();
        let lossy = ed.open_file(&path).unwrap();
        assert!(!lossy);
        assert_eq!(ed.active_editor().text(), "one\ntwo\n");
        assert_eq!(ed.path(), Some(path.as_path()));
        assert!(!ed.is_modified(), "a freshly opened file is clean");

        // Edit then save: the CRLF style is preserved on disk.
        keydown(&mut ed, KeyCode::Char('!'), Modifiers::NONE); // insert at (0,0)
        assert!(ed.is_modified());
        ed.save_to(&path).unwrap();
        assert!(!ed.is_modified());
        assert_eq!(std::fs::read(&path).unwrap(), b"!one\r\ntwo\r\n");

        std::fs::remove_file(&path).ok();
    }

    // --- MDI: windows (Phase 8a.2) ---

    const THEME: fn() -> Theme = Theme::default;

    #[test]
    fn a_fresh_app_has_one_active_window() {
        let ed = app();
        assert_eq!(ed.window_count(), 1);
        assert_eq!(ed.active_index(), 0);
    }

    #[test]
    fn new_window_adds_an_empty_document_and_activates_it() {
        let mut ed = app();
        type_chars(&mut ed, "first"); // window 0 has content
        ed.new_window(&THEME());
        assert_eq!(ed.window_count(), 2);
        assert_eq!(ed.active_index(), 1);
        assert_eq!(ed.active_editor().text(), "", "the new window is blank");
        // Switching back finds the first window's text untouched.
        ed.activate(0);
        assert_eq!(ed.active_editor().text(), "first");
    }

    #[test]
    fn new_menu_command_spawns_a_window_rather_than_replacing() {
        let mut ed = app();
        keydown(&mut ed, KeyCode::Char('f'), Modifiers::ALT); // open File
        let posted = keydown(&mut ed, KeyCode::Enter, Modifiers::NONE); // first item: New
        assert_eq!(posted, vec![CM_NEW]);
    }

    #[test]
    fn typing_targets_only_the_active_window() {
        let mut ed = app();
        ed.new_window(&THEME());
        type_chars(&mut ed, "two");
        ed.activate(0);
        assert_eq!(ed.active_editor().text(), "", "window 0 stayed empty");
        ed.activate(1);
        assert_eq!(ed.active_editor().text(), "two");
    }

    #[test]
    fn alt_digit_switches_to_that_window() {
        let mut ed = app();
        ed.new_window(&THEME()); // index 1
        ed.new_window(&THEME()); // index 2
        keydown(&mut ed, KeyCode::Char('1'), Modifiers::ALT);
        assert_eq!(ed.active_index(), 0);
        keydown(&mut ed, KeyCode::Char('3'), Modifiers::ALT);
        assert_eq!(ed.active_index(), 2);
        // Out-of-range digit is a no-op (only three windows).
        keydown(&mut ed, KeyCode::Char('9'), Modifiers::ALT);
        assert_eq!(ed.active_index(), 2);
    }

    #[test]
    fn f6_and_shift_f6_cycle_windows_with_wraparound() {
        let mut ed = app();
        ed.new_window(&THEME()); // index 1
        ed.new_window(&THEME()); // index 2, active
        keydown(&mut ed, KeyCode::F(6), Modifiers::NONE); // wraps 2 -> 0
        assert_eq!(ed.active_index(), 0);
        keydown(&mut ed, KeyCode::F(6), Modifiers::SHIFT); // back 0 -> 2
        assert_eq!(ed.active_index(), 2);
    }

    #[test]
    fn the_window_menu_posts_its_commands() {
        // The Window menu, in order: Next, Previous, Zoom, Cascade, Tile, Close.
        let expected = [
            CM_NEXT_WINDOW,
            CM_PREV_WINDOW,
            CM_ZOOM,
            CM_CASCADE,
            CM_TILE,
            CM_CLOSE,
        ];
        for (steps, &cmd) in expected.iter().enumerate() {
            let mut ed = app();
            keydown(&mut ed, KeyCode::Char('w'), Modifiers::ALT); // open Window (Next highlighted)
            assert!(ed.menu_is_open());
            for _ in 0..steps {
                keydown(&mut ed, KeyCode::Down, Modifiers::NONE);
            }
            assert_eq!(keydown(&mut ed, KeyCode::Enter, Modifiers::NONE), vec![cmd]);
        }
    }

    #[test]
    fn alt_f3_posts_close_but_f3_alone_does_not() {
        let mut ed = app();
        assert_eq!(
            keydown(&mut ed, KeyCode::F(3), Modifiers::ALT),
            vec![CM_CLOSE]
        );
        // F3 alone is the editor's Find Next; it posts no window command.
        assert!(keydown(&mut ed, KeyCode::F(3), Modifiers::NONE).is_empty());
    }

    #[test]
    fn removing_a_window_activates_a_neighbour() {
        let mut ed = app();
        ed.new_window(&THEME()); // 1
        ed.new_window(&THEME()); // 2, active
        ed.remove_active_window();
        assert_eq!(ed.window_count(), 2);
        assert_eq!(ed.active_index(), 1, "the previous window becomes active");
    }

    #[test]
    fn closing_the_last_window_leaves_a_fresh_untitled() {
        let mut ed = app();
        type_chars(&mut ed, "stuff");
        ed.documents[ed.active].path = Some(PathBuf::from("/tmp/x.txt"));
        ed.remove_active_window();
        assert_eq!(ed.window_count(), 1, "never fewer than one window");
        assert_eq!(ed.active_editor().text(), "");
        assert!(ed.path().is_none());
        assert!(!ed.is_modified());
    }

    #[test]
    fn an_open_menu_swallows_window_keys() {
        let mut ed = app();
        ed.new_window(&THEME()); // index 1, active
        keydown(&mut ed, KeyCode::Char('f'), Modifiers::ALT); // open File
        keydown(&mut ed, KeyCode::F(6), Modifiers::NONE); // should be swallowed by the menu
        assert_eq!(ed.active_index(), 1, "F6 never reached window switching");
    }

    // --- MDI: layout — zoom / cascade / tile (Phase 8b) ---

    #[test]
    fn a_fresh_app_starts_zoomed_and_maximised() {
        let ed = app();
        assert!(ed.zoomed);
        assert_eq!(
            ed.active_editor().bounds(),
            inset1(regions(ed.size).desktop)
        );
    }

    #[test]
    fn switching_windows_while_zoomed_maximises_the_new_active() {
        let mut ed = app();
        ed.new_window(&THEME());
        let maximised = inset1(regions(ed.size).desktop);
        ed.activate(0);
        assert_eq!(ed.active_editor().bounds(), maximised);
        ed.activate(1);
        assert_eq!(ed.active_editor().bounds(), maximised);
    }

    #[test]
    fn f5_zoom_toggles_the_active_window_between_maximised_and_normal() {
        let mut ed = app();
        ed.new_window(&THEME());
        let maximised = ed.active_editor().bounds();
        ed.cascade(); // un-zoom: windows take their cascade slots
        assert!(!ed.zoomed);
        assert_ne!(
            ed.active_editor().bounds(),
            maximised,
            "a cascaded window is not maximised"
        );
        keydown(&mut ed, KeyCode::F(5), Modifiers::NONE); // Zoom back on
        assert!(ed.zoomed);
        assert_eq!(ed.active_editor().bounds(), maximised);
    }

    #[test]
    fn cascade_offsets_each_window_down_and_right() {
        let mut ed = app();
        ed.new_window(&THEME());
        ed.new_window(&THEME());
        ed.cascade();
        let o0 = ed.documents[0].normal.origin();
        let o1 = ed.documents[1].normal.origin();
        let o2 = ed.documents[2].normal.origin();
        assert!(o1.x > o0.x && o1.y > o0.y);
        assert!(o2.x > o1.x && o2.y > o1.y);
    }

    #[test]
    fn tile_two_windows_splits_the_desktop_in_half() {
        let mut ed = EditorApp::new(Size::new(40, 12), &THEME());
        ed.new_window(&THEME());
        ed.tile();
        assert!(!ed.zoomed);
        let ds = regions(ed.size).desktop.size();
        let a = ed.documents[0].normal;
        let b = ed.documents[1].normal;
        // Side by side, full height, together covering the whole desktop width.
        assert_eq!(a.origin(), Point::new(0, 0));
        assert_eq!(a.height(), ds.height);
        assert_eq!(b.origin(), Point::new(a.width(), 0));
        assert_eq!(a.width() + b.width(), ds.width);
        assert_eq!(b.height(), ds.height);
    }

    #[test]
    fn snapshot_cascaded_windows_overlap_on_the_desktop() {
        use rvision::buffer::Buffer;
        use rvision::canvas::Canvas;

        let mut ed = EditorApp::new(Size::new(34, 12), &THEME());
        ed.active_editor_mut().set_text("first window");
        ed.new_window(&THEME());
        ed.active_editor_mut().set_text("second window");
        ed.cascade();
        let mut buf = Buffer::new(Size::new(34, 12));
        ed.draw_canvas(&mut Canvas::new(&mut buf));
        insta::assert_snapshot!(buf.to_text());
    }
}
