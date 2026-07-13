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
use rvision::arrange::{self, ChromeFlags, ChromeHit};
use rvision::backend::{Backend, EventSource};
use rvision::buffer::Buffer;
use rvision::canvas::Canvas;
use rvision::cell::Cell;
use rvision::color::Style;
use rvision::command::{
    Accelerator, CM_HELP, CM_NO, CM_OK, CM_QUIT, CM_USER, CM_YES, Command, CommandSet,
};
use rvision::event::{
    Event, EventResult, KeyCode, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseKind,
};
use rvision::geometry::{Point, Rect, Size};
use rvision::help::HelpContents;
use rvision::theme::{Role, Theme};
use rvision::view::{Context, View};
use rvision::widgets::{
    FileDialog, Frame, HelpWindow, Menu, MenuBar, MenuItem, MessageBox, Orientation, ScrollBar,
    ScrollPart, StatusItem, StatusLine,
};

use crate::dialogs::{CM_DEFAULTS, FindDialog, GoToLine, ReplaceDialog, SettingsDialog};
use crate::editor::{
    CM_COPY, CM_CUT, CM_FIND, CM_FIND_NEXT, CM_GOTO, CM_PASTE, CM_REDO, CM_REPLACE, CM_UNDO,
    EditorView,
};
use crate::file::{self, Encoding};
use crate::help::HELP_TEXT;
use crate::settings::{MAX_RECENT, Settings};

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

/// Help ▸ About — show the About box. Works on an empty desktop (needs no
/// document), so it sits in the `handle_command` allowlist alongside New/Open.
pub const CM_ABOUT: Command = Command(CM_USER + 40);

/// Edit ▸ Settings — open the Settings dialog. Needs no document, so it is in the
/// empty-desktop allowlist too.
pub const CM_SETTINGS: Command = Command(CM_USER + 41);

/// First of the File menu's recent-files commands (ADR 0025). The `k`-th entry
/// posts `Command(CM_RECENT_BASE.0 + k)`; the block spans [`MAX_RECENT`] ids,
/// disjoint from every other command space above.
pub const CM_RECENT_BASE: Command = Command(CM_USER + 50);

/// The MRU index a recent-files command refers to, or `None` if it is not one.
fn recent_index(command: Command) -> Option<usize> {
    let base = CM_RECENT_BASE.0;
    let id = command.0;
    (base..base + MAX_RECENT as u16)
        .contains(&id)
        .then(|| (id - base) as usize)
}

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

    /// The window's base name — the file name, or `Untitled` for an unsaved
    /// document. The `*` modified marker and any ` (n)` instance indicator for
    /// duplicate names are added by [`EditorApp::window_title`].
    fn base_name(&self) -> String {
        match &self.path {
            Some(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.to_string_lossy().into_owned()),
            None => "Untitled".to_string(),
        }
    }

    /// The directory a file dialog should start in: this file's folder, or the
    /// process working directory.
    fn start_dir(&self) -> PathBuf {
        self.path
            .as_ref()
            .and_then(|p| p.parent())
            // `Path::parent()` returns `Some("")`, not `None`, for a single-component
            // relative path (e.g. the `edit somename.txt` command-line case) — treat
            // that the same as "no parent" rather than handing an empty path to a
            // file dialog, which can't list it.
            .filter(|p| !p.as_os_str().is_empty())
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
    /// Persisted preferences: tab width, Find options, and the recent-files MRU
    /// (ADR 0025). Loaded at startup and saved back when one of them changes.
    settings: Settings,
    finished: bool,
    /// An in-progress drag of the active window's chrome (Phase 9d), or `None`.
    drag: Option<Drag>,
    /// Whether the in-progress left-drag is extending an editor selection — set
    /// only when a press actually landed in the editor interior, so a drag that
    /// began on a scroll bar or the frame never leaks into a text selection.
    selecting: bool,
    frame_style: Style,
    title_style: Style,
    inactive_title_style: Style,
    shadow_style: Style,
    backdrop: Cell,
    /// The help window, if open — a standalone singleton overlay (ADR 0027),
    /// not a member of `documents`: it never joins the Window menu, F6 cycle,
    /// or Alt+1..9, but is otherwise a real resident window (move/resize/close).
    help: Option<HelpOverlay>,
    /// Whether `help` (rather than the document stack) currently owns the
    /// keyboard and draws on top. Meaningless while `help` is `None`.
    help_focused: bool,
}

/// The resident help window (ADR 0027): rvision's own [`rvision::widgets::Window`]
/// (built by [`HelpWindow::build`]/`build_at`), hosted directly by `EditorApp`
/// instead of `exec_view`, so its bounds live in the same desktop-local
/// coordinate space `Document::normal` uses.
struct HelpOverlay {
    window: rvision::widgets::Window,
    /// An in-progress drag of the help window's own chrome, or `None`.
    drag: Option<HelpDrag>,
}

/// An in-progress title-bar move or corner resize of the help window's frame,
/// in desktop-local coordinates — the same shape as [`Drag`]'s `Session`
/// case, minus `ScrollThumb` (the help window handles its own internal
/// scroll-bar dragging, ADR 0027) and any zoom variant (help is never
/// zoomable), so an [`arrange::ArrangeSession`] alone is the whole story
/// (ADR 0028).
type HelpDrag = arrange::ArrangeSession;

/// An in-progress mouse drag of the active window's frame (Phase 9d).
#[derive(Debug, Clone, Copy)]
enum Drag {
    /// A title-bar move or corner resize, in desktop-local coordinates
    /// (ADR 0028).
    Session(arrange::ArrangeSession),
    /// Dragging a scroll-bar thumb: the bar's axis. The active window scrolls so
    /// its thumb follows the pointer (`ScrollBar::pos_at` inverts the placement).
    ScrollThumb { orientation: Orientation },
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

/// The vertical scroll bar's rectangle within a window of `size`: the right
/// border between the top and bottom corners, origin-relative to the window.
/// Shared by drawing and mouse hit-testing so they can't drift.
fn vscroll_rect(size: Size) -> Rect {
    Rect::from_origin_size(Point::new(size.width - 1, 1), Size::new(1, size.height - 2))
}

/// The horizontal scroll bar's rectangle within a window of `size`: the bottom
/// border between the left and right corners, origin-relative to the window.
fn hscroll_rect(size: Size) -> Rect {
    Rect::from_origin_size(Point::new(1, size.height - 1), Size::new(size.width - 2, 1))
}

/// The smallest window that still has a usable interior (a 1-cell border plus at
/// least one interior cell). `rvision::arrange`'s own functions take this as a
/// caller-supplied `min_size` rather than a hardcoded floor, since `Desktop` and
/// `edit` have always disagreed on it (ADR 0028).
const MIN_WINDOW: Size = Size::new(3, 3);

/// The command posted by the `index`-th recent-files menu entry.
fn recent_command(index: usize) -> Command {
    Command(CM_RECENT_BASE.0 + index as u16)
}

/// The File-menu label for the `index`-th recent file: a 1-based number and the
/// file's name (falling back to the whole path when it has no final component).
fn recent_label(index: usize, path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    format!("{} {}", index + 1, name)
}

/// Builds the menu bar for a terminal of `size`, splicing the `recent` files
/// (newest first) into the File menu between "Save As..." and "Exit". Rebuilt
/// from scratch whenever the MRU changes, since `MenuBar` has no post-build
/// mutation API and the menus are cheap, plain data.
fn build_menu_bar(size: Size, theme: &Theme, recent: &[PathBuf]) -> MenuBar {
    let mut file = vec![
        MenuItem::new("New", CM_NEW),
        MenuItem::new("Open...", CM_OPEN),
        MenuItem::new("Save", CM_SAVE).with_shortcut("F2"),
        MenuItem::new("Save As...", CM_SAVE_AS).with_hotkey('a'),
    ];
    for (i, path) in recent.iter().take(MAX_RECENT).enumerate() {
        file.push(MenuItem::new(&recent_label(i, path), recent_command(i)));
    }
    file.push(
        MenuItem::new("Exit", CM_QUIT)
            .with_shortcut("Alt-X")
            .with_hotkey('x'),
    );

    MenuBar::new(
        regions(size).menu,
        vec![
            Menu::new("File", file),
            Menu::new(
                "Edit",
                vec![
                    MenuItem::new("Undo", CM_UNDO).with_shortcut("Ctrl-Z"),
                    MenuItem::new("Redo", CM_REDO).with_shortcut("Ctrl-Y"),
                    MenuItem::new("Cut", CM_CUT)
                        .with_shortcut("Ctrl-X")
                        .with_hotkey('t'),
                    MenuItem::new("Copy", CM_COPY).with_shortcut("Ctrl-C"),
                    MenuItem::new("Paste", CM_PASTE).with_shortcut("Ctrl-V"),
                    MenuItem::new("Settings...", CM_SETTINGS),
                ],
            ),
            Menu::new(
                "Search",
                vec![
                    MenuItem::new("Find...", CM_FIND).with_shortcut("Ctrl-F"),
                    MenuItem::new("Find Next", CM_FIND_NEXT)
                        .with_shortcut("F3")
                        .with_hotkey('n'),
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
                    MenuItem::new("Close", CM_CLOSE)
                        .with_shortcut("Alt-F3")
                        .with_hotkey('l'),
                ],
            ),
            Menu::new(
                "Help",
                vec![
                    MenuItem::new("Help Topics", CM_HELP).with_shortcut("F1"),
                    MenuItem::new("About...", CM_ABOUT),
                ],
            ),
        ],
        theme,
    )
}

impl EditorApp {
    /// Builds the editor application for a terminal of `size` with an empty,
    /// unsaved document.
    pub fn new(size: Size, theme: &Theme) -> Self {
        let menu_bar = build_menu_bar(size, theme, &[]);
        let status_line = StatusLine::new(
            regions(size).status,
            vec![
                StatusItem::new(
                    "F1",
                    "Help",
                    Accelerator::new(KeyEvent::new(KeyCode::F(1), Modifiers::NONE), CM_HELP),
                ),
                StatusItem::new(
                    "F2",
                    "Save",
                    Accelerator::new(KeyEvent::new(KeyCode::F(2), Modifiers::NONE), CM_SAVE),
                ),
                // F3 is the editor's Find Next (consumed before the status line),
                // so Open lives on the File menu; no F3 accelerator here.
                StatusItem::new(
                    "Alt-X",
                    "Exit",
                    Accelerator::new(KeyEvent::new(KeyCode::Char('x'), Modifiers::ALT), CM_QUIT),
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
            settings: Settings::default(),
            finished: false,
            drag: None,
            selecting: false,
            frame_style: theme.style(Role::WindowFrame),
            title_style: theme.style(Role::WindowTitle),
            inactive_title_style: theme.style(Role::WindowTitleInactive),
            shadow_style: theme.style(Role::Shadow),
            backdrop: Cell::from_char('░', theme.style(Role::DesktopBackground)),
            help: None,
            help_focused: false,
        };
        app.add_document(Document::new(theme));
        app
    }

    /// Adopts the persisted `settings`: re-applies the tab width to existing
    /// documents and rebuilds the File menu's recent-files list. Call once at
    /// startup, after [`Settings::load`] (ADR 0025).
    pub fn apply_settings(&mut self, settings: Settings, theme: &Theme) {
        self.settings = settings;
        for doc in &mut self.documents {
            doc.editor.set_tab_width(self.settings.tab_width);
        }
        self.menu_bar = build_menu_bar(self.size, theme, &self.settings.recent);
    }

    /// The persisted preferences (e.g. to seed a dialog from the saved Find options).
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Records the active document's path at the front of the recent-files list,
    /// rebuilds the File menu, and persists the settings. A no-op for an unsaved
    /// (pathless) document.
    pub fn note_recent(&mut self, theme: &Theme) {
        let Some(path) = self.documents.get(self.active).and_then(|d| d.path.clone()) else {
            return;
        };
        self.settings.record_recent(path);
        self.menu_bar = build_menu_bar(self.size, theme, &self.settings.recent);
        let _ = self.settings.save();
    }

    /// Persists changed Find options (case / whole-word) so the next Find or
    /// Replace dialog opens the way the user last left it. No menu rebuild — the
    /// options don't appear there.
    fn remember_find_options(&mut self, case_sensitive: bool, whole_word: bool) {
        if case_sensitive != self.settings.find_case_sensitive
            || whole_word != self.settings.find_whole_word
        {
            self.settings.find_case_sensitive = case_sensitive;
            self.settings.find_whole_word = whole_word;
            let _ = self.settings.save();
        }
    }

    /// The recent file at `index`, if the list still has one there.
    fn recent_path(&self, index: usize) -> Option<PathBuf> {
        self.settings.recent.get(index).cloned()
    }

    /// Drops a recent path that failed to open (it was deleted or moved),
    /// rebuilds the File menu, and persists.
    fn forget_recent(&mut self, path: &Path, theme: &Theme) {
        self.settings.recent.retain(|p| p != path);
        self.menu_bar = build_menu_bar(self.size, theme, &self.settings.recent);
        let _ = self.settings.save();
    }

    /// Re-applies the tab width to every open document, rebuilds the File menu (the
    /// recent list may have been re-capped), and persists the settings — the shared
    /// tail of the Settings-dialog actions.
    fn refresh_after_settings_change(&mut self, theme: &Theme) {
        for doc in &mut self.documents {
            doc.editor.set_tab_width(self.settings.tab_width);
        }
        self.menu_bar = build_menu_bar(self.size, theme, &self.settings.recent);
        let _ = self.settings.save();
    }

    /// Applies the Settings dialog's values — each `None` keeps the current value
    /// (an empty or non-numeric field) — then refreshes and persists.
    pub fn apply_settings_from_dialog(
        &mut self,
        tab_width: Option<usize>,
        recent_limit: Option<usize>,
        theme: &Theme,
    ) {
        if let Some(width) = tab_width {
            self.settings.set_tab_width(width);
        }
        if let Some(limit) = recent_limit {
            self.settings.set_recent_limit(limit);
        }
        self.refresh_after_settings_change(theme);
    }

    /// Resets every preference to its default (keeping the recent-files history),
    /// then refreshes and persists — the dialog's "Reset to defaults" (ADR 0025).
    pub fn reset_settings_to_defaults(&mut self, theme: &Theme) {
        self.settings.reset_keeping_recent();
        self.refresh_after_settings_change(theme);
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
            doc.normal = arrange::clamp_rect(doc.normal, ds);
        }
        if let Some(help) = &mut self.help {
            help.window
                .set_bounds(arrange::clamp_rect(help.window.bounds(), ds));
        }
        self.sync_layout();
    }

    /// The desktop area (screen coordinates) the windows live in.
    fn desktop(&self) -> Rect {
        regions(self.size).desktop
    }

    /// Converts a screen-space point to desktop-local coordinates — the space
    /// `Document::normal`/`HelpOverlay::window`'s bounds already use, and the
    /// space a drag session's anchor/bounds are tracked in (ADR 0028).
    fn to_desktop_local(&self, pos: Point) -> Point {
        let origin = self.desktop().origin();
        pos.offset(-origin.x, -origin.y)
    }

    /// Window `i`'s effective rectangle in **desktop-local** coordinates: its
    /// `normal` rect, unless it is the zoomed active window, which fills the
    /// desktop. Always clamped to the desktop.
    fn window_rect_local(&self, i: usize) -> Rect {
        let ds = self.desktop().size();
        if self.zoomed && i == self.active {
            Rect::from_origin_size(Point::new(0, 0), ds)
        } else {
            arrange::clamp_rect(self.documents[i].normal, ds)
        }
    }

    /// Window `i`'s effective rectangle in **screen** coordinates (its desktop-local
    /// rect shifted by the desktop origin).
    fn window_rect_screen(&self, i: usize) -> Rect {
        let origin = self.desktop().origin();
        let local = self.window_rect_local(i);
        Rect::from_origin_size(local.origin().offset(origin.x, origin.y), local.size())
    }

    /// The topmost window whose screen rectangle contains `pos`, or `None` if the
    /// point is over bare desktop. Searches front-to-back (the reverse of
    /// [`draw_order`](Self::draw_order)), so the visually-uppermost window wins.
    fn window_at(&self, pos: Point) -> Option<usize> {
        self.draw_order().into_iter().rev().find(|&i| {
            !self.window_rect_screen(i).is_empty() && self.window_rect_screen(i).contains(pos)
        })
    }

    /// If screen point `pos` is on window `i`'s scroll bar, which bar and which part
    /// of it. Rebuilds the bar with the editor's current metrics (the same way
    /// [`draw_scrollbars`](Self::draw_scrollbars) does) so the click target matches
    /// what is drawn.
    fn scrollbar_hit(&self, i: usize, pos: Point) -> Option<(Orientation, ScrollPart)> {
        let area = self.window_rect_screen(i);
        let o = area.origin();
        let m = self.documents[i].editor.scroll_metrics();
        let screen = |local: Rect| {
            Rect::from_origin_size(o.offset(local.origin().x, local.origin().y), local.size())
        };

        let vbar = screen(vscroll_rect(area.size()));
        if m.needs_vertical() && !vbar.is_empty() && vbar.contains(pos) {
            let mut bar = ScrollBar::new(vbar, self.frame_style);
            bar.set_metrics(m.lines, m.viewport.height.max(0) as usize, m.top);
            return bar.hit(pos).map(|part| (Orientation::Vertical, part));
        }
        let hbar = screen(hscroll_rect(area.size()));
        if m.needs_horizontal() && !hbar.is_empty() && hbar.contains(pos) {
            let mut bar = ScrollBar::horizontal(hbar, self.frame_style);
            bar.set_metrics(
                m.content_width.max(0) as usize,
                m.viewport.width.max(0) as usize,
                m.left.max(0) as usize,
            );
            return bar.hit(pos).map(|part| (Orientation::Horizontal, part));
        }
        None
    }

    /// Applies a scroll-bar click on window `i`'s editor: the arrows step a line/
    /// column, the track pages by the viewport extent, the thumb does nothing yet
    /// (dragging is Phase 9d).
    fn apply_scroll(&mut self, i: usize, orientation: Orientation, part: ScrollPart) {
        let m = self.documents[i].editor.scroll_metrics();
        let editor = &mut self.documents[i].editor;
        match orientation {
            Orientation::Vertical => {
                let page = m.viewport.height.max(1);
                match part {
                    ScrollPart::LineUp => editor.scroll_lines(-1),
                    ScrollPart::LineDown => editor.scroll_lines(1),
                    ScrollPart::PageUp => editor.scroll_lines(-page),
                    ScrollPart::PageDown => editor.scroll_lines(page),
                    ScrollPart::Thumb => {}
                }
            }
            Orientation::Horizontal => {
                let page = m.viewport.width.max(1);
                match part {
                    ScrollPart::LineUp => editor.scroll_cols(-1),
                    ScrollPart::LineDown => editor.scroll_cols(1),
                    ScrollPart::PageUp => editor.scroll_cols(-page),
                    ScrollPart::PageDown => editor.scroll_cols(page),
                    ScrollPart::Thumb => {}
                }
            }
        }
    }

    // --- window chrome drag (Phase 9d) ---

    /// Classifies a press at screen `pos` on window `i`'s chrome: the close/zoom
    /// glyphs (column spans shared with the [`Frame`] drawing), the bottom-right
    /// corner (resize), or the rest of the title row (move). Documents are
    /// always zoomable.
    fn chrome_hit(&self, i: usize, pos: Point) -> ChromeHit {
        Self::chrome_hit_at(self.window_rect_screen(i), pos, true)
    }

    /// The generic geometry [`chrome_hit`](Self::chrome_hit) reduces to: pure
    /// `Rect` math with no `Document` access, so the help overlay (ADR 0027)
    /// reuses it directly against its own screen rect. `zoomable` gates
    /// whether the zoom-glyph span is tested at all — the help window has no
    /// zoom glyph to hit. `edit`'s windows are always moveable/resizable/
    /// closable and never draw a context-help glyph (ADR 0021's `F1` glyph is
    /// a `rvision::widgets::Window` feature this bespoke chrome doesn't use,
    /// ADR 0028) — only `zoomable` varies between a `Document` and help.
    fn chrome_hit_at(r: Rect, pos: Point, zoomable: bool) -> ChromeHit {
        arrange::chrome_hit(
            r,
            pos,
            ChromeFlags {
                moveable: true,
                resizable: true,
                closable: true,
                zoomable,
                has_help: false,
            },
        )
    }

    /// Restores a maximised active window to the full desktop as its `normal` rect
    /// before a drag, so move/resize work from a concrete rectangle (and the window
    /// does not jump). A no-op when not zoomed.
    fn unzoom_for_drag(&mut self) {
        if self.zoomed {
            let ds = self.desktop().size();
            self.documents[self.active].normal = Rect::from_origin_size(Point::new(0, 0), ds);
            self.zoomed = false;
            self.sync_layout();
        }
    }

    /// Begins a title-bar move drag, anchored at the desktop-local point the
    /// pointer grabbed so it tracks without jumping.
    fn start_move(&mut self, pos: Point) {
        self.unzoom_for_drag();
        let bounds = self.window_rect_local(self.active);
        let anchor = self.to_desktop_local(pos);
        self.drag = Some(Drag::Session(arrange::start_session(
            arrange::ArrangeKind::Move,
            bounds,
            anchor,
        )));
    }

    /// Begins a corner resize drag, anchored at the desktop-local point the
    /// pointer grabbed.
    fn start_resize(&mut self, pos: Point) {
        self.unzoom_for_drag();
        let bounds = self.window_rect_local(self.active);
        let anchor = self.to_desktop_local(pos);
        self.drag = Some(Drag::Session(arrange::start_session(
            arrange::ArrangeKind::Resize,
            bounds,
            anchor,
        )));
    }

    /// Advances the in-progress drag to screen `pos`: moves the window's origin or
    /// resizes from its corner, clamped to the desktop (and a minimum size), then
    /// re-syncs editor bounds. A no-op when no drag is active.
    fn drag_to(&mut self, pos: Point) {
        let Some(drag) = self.drag else {
            return;
        };
        match drag {
            Drag::Session(session) => {
                let ds = self.desktop().size();
                let local_pos = self.to_desktop_local(pos);
                let next = arrange::continue_session(&session, local_pos, MIN_WINDOW);
                self.documents[self.active].normal = arrange::clamp_rect(next, ds);
                self.sync_layout();
            }
            // A thumb drag scrolls the editor rather than moving the window.
            Drag::ScrollThumb { orientation } => {
                self.scroll_to_thumb(self.active, orientation, pos);
            }
        }
    }

    /// Scrolls window `i`'s editor so the dragged scroll-bar thumb tracks screen
    /// `pos`: rebuilds the bar with the editor's current metrics (the geometry
    /// shared with drawing and hit-testing) and steps to the position
    /// [`ScrollBar::pos_at`] reports under the pointer.
    fn scroll_to_thumb(&mut self, i: usize, orientation: Orientation, pos: Point) {
        let area = self.window_rect_screen(i);
        let m = self.documents[i].editor.scroll_metrics();
        let o = area.origin();
        let screen = |local: Rect| {
            Rect::from_origin_size(o.offset(local.origin().x, local.origin().y), local.size())
        };
        let editor = &mut self.documents[i].editor;
        match orientation {
            Orientation::Vertical => {
                let mut bar = ScrollBar::new(screen(vscroll_rect(area.size())), self.frame_style);
                bar.set_metrics(m.lines, m.viewport.height.max(0) as usize, m.top);
                editor.scroll_lines(bar.pos_at(pos) as i16 - m.top as i16);
            }
            Orientation::Horizontal => {
                let mut bar =
                    ScrollBar::horizontal(screen(hscroll_rect(area.size())), self.frame_style);
                bar.set_metrics(
                    m.content_width.max(0) as usize,
                    m.viewport.width.max(0) as usize,
                    m.left.max(0) as usize,
                );
                editor.scroll_cols(bar.pos_at(pos) as i16 - m.left);
            }
        }
    }

    // --- help overlay (ADR 0027): a standalone, non-modal resident window ---

    /// Opens the help window at `initial` (or the home topic when `None`), or —
    /// if one is already open — just brings it to front and gives it focus,
    /// ignoring `initial` (a singleton, reused rather than rebuilt, matching
    /// `rvision`'s own "hold a window by value" idiom, ADR 0016). The `initial`
    /// parameter is the context-sensitivity seam (ADR 0023): every caller
    /// passes `None` for now; later a dialog or control can name a relevant
    /// topic.
    pub fn open_help(&mut self, theme: &Theme, initial: Option<&str>) {
        if self.help.is_none() {
            let contents = HelpContents::parse(HELP_TEXT);
            let area = Rect::from_origin_size(Point::new(0, 0), self.desktop().size());
            let window = match initial {
                Some(topic) => HelpWindow::build_at(contents, area, "Help", theme, topic),
                None => HelpWindow::build(contents, area, "Help", theme),
            }
            .moveable(true)
            .resizable(true)
            .closable(true)
            .zoomable(false);
            self.help = Some(HelpOverlay { window, drag: None });
        }
        self.focus_help();
    }

    /// Whether the help window is open *and* currently owns the keyboard/topmost
    /// draw position, rather than the document stack.
    pub(crate) fn help_focused(&self) -> bool {
        self.help_focused && self.help.is_some()
    }

    /// Gives the help window focus (keyboard + topmost), syncing its own
    /// active/inactive frame style to match. A no-op if it isn't open.
    fn focus_help(&mut self) {
        self.help_focused = true;
        if let Some(help) = &mut self.help {
            help.window.set_active(true);
        }
    }

    /// Hands focus back to the document stack, syncing the help window's
    /// frame style to inactive. A no-op if it isn't open.
    fn defocus_help(&mut self) {
        self.help_focused = false;
        if let Some(help) = &mut self.help {
            help.window.set_active(false);
        }
    }

    /// Closes the help window outright — no discard guard, since it has
    /// nothing to save (unlike [`remove_active_window`](Self::remove_active_window)).
    pub(crate) fn close_help(&mut self) {
        self.help = None;
    }

    /// The help window's effective rectangle in **screen** coordinates, or
    /// `None` if it isn't open — mirrors [`window_rect_screen`](Self::window_rect_screen).
    fn help_rect_screen(&self) -> Option<Rect> {
        let help = self.help.as_ref()?;
        let origin = self.desktop().origin();
        let local = help.window.bounds();
        Some(Rect::from_origin_size(
            local.origin().offset(origin.x, origin.y),
            local.size(),
        ))
    }

    /// Begins a title-bar move drag of the help window.
    fn start_help_move(&mut self, pos: Point) {
        let anchor = self.to_desktop_local(pos);
        if let Some(help) = &mut self.help {
            let bounds = help.window.bounds();
            help.drag = Some(arrange::start_session(
                arrange::ArrangeKind::Move,
                bounds,
                anchor,
            ));
        }
    }

    /// Begins a corner resize drag of the help window.
    fn start_help_resize(&mut self, pos: Point) {
        let anchor = self.to_desktop_local(pos);
        if let Some(help) = &mut self.help {
            let bounds = help.window.bounds();
            help.drag = Some(arrange::start_session(
                arrange::ArrangeKind::Resize,
                bounds,
                anchor,
            ));
        }
    }

    /// Advances the help window's in-progress drag to screen `pos`, clamped to
    /// the desktop (and a minimum size) — mirrors [`drag_to`](Self::drag_to).
    fn drag_help_to(&mut self, pos: Point) {
        let ds = self.desktop().size();
        let local_pos = self.to_desktop_local(pos);
        let Some(help) = &mut self.help else { return };
        let Some(session) = help.drag else { return };
        let next = arrange::continue_session(&session, local_pos, MIN_WINDOW);
        help.window.set_bounds(arrange::clamp_rect(next, ds));
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
                arrange::clamp_rect(self.documents[i].normal, ds)
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

    /// Which commands are currently live, from document state: undo/redo
    /// availability, selection, the dirty flag, and whether any window is open at
    /// all. `dispatch` gates on this (a disabled command's key or menu selection
    /// is a no-op, ADR 0003); [`sync_menu`](Self::sync_menu) pushes it (plus one
    /// Paste-specific tweak) into the menu bar so greying and gating agree by
    /// construction (ADR 0004).
    fn command_set(&self) -> CommandSet {
        let mut commands = CommandSet::new();
        if self.documents.is_empty() {
            // Nothing to act on: only the empty-desktop allowlist in
            // `handle_command` (New/Open/Quit/About/Help/Settings, plus a recent
            // entry) does anything.
            for command in [
                CM_SAVE,
                CM_SAVE_AS,
                CM_UNDO,
                CM_REDO,
                CM_CUT,
                CM_COPY,
                CM_PASTE,
                CM_FIND,
                CM_FIND_NEXT,
                CM_REPLACE,
                CM_GOTO,
                CM_CLOSE,
                CM_NEXT_WINDOW,
                CM_PREV_WINDOW,
                CM_CASCADE,
                CM_TILE,
                CM_ZOOM,
            ] {
                commands.disable(command);
            }
            return commands;
        }
        let editor = self.active_editor();
        if !editor.can_undo() {
            commands.disable(CM_UNDO);
        }
        if !editor.can_redo() {
            commands.disable(CM_REDO);
        }
        if !editor.has_selection() {
            commands.disable(CM_CUT);
            commands.disable(CM_COPY);
        }
        if !editor.is_modified() {
            commands.disable(CM_SAVE);
        }
        commands
    }

    /// Pushes the live command state into the menu bar before a draw, so a
    /// disabled item greys itself (`MenuBar::sync_enabled`, mirrors the
    /// `View::set_focused` state-in-draw push, ADR 0004).
    ///
    /// Paste is the one place this diverges from [`command_set`](Self::command_set):
    /// an empty clipboard greys the menu entry, but must not disable `CM_PASTE` in
    /// `dispatch`'s `CommandSet`, or Ctrl-V would silently do nothing instead of
    /// reaching the "clipboard is empty" explainer in `handle_command` (ADR 0021/0022).
    fn sync_menu(&mut self) {
        let mut commands = self.command_set();
        if self.clipboard.is_empty() {
            commands.disable(CM_PASTE);
        }
        self.menu_bar.sync_enabled(&commands);
    }

    /// Whether the loop should stop.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// The active document's file path, if any.
    pub fn path(&self) -> Option<&Path> {
        self.doc().path.as_deref()
    }

    /// The active window's title (with any instance indicator and modified `*`).
    fn title(&self) -> String {
        self.window_title(self.active)
    }

    /// The title for window `i`: its base name, plus a ` (n)` instance indicator
    /// when more than one window shares that base name (numbered in window order),
    /// plus a trailing `*` when the document is modified.
    fn window_title(&self, i: usize) -> String {
        let base = self.documents[i].base_name();
        let same: Vec<usize> = (0..self.documents.len())
            .filter(|&j| self.documents[j].base_name() == base)
            .collect();
        let mut title = base;
        if same.len() > 1 {
            let ordinal = same.iter().position(|&j| j == i).unwrap() + 1;
            title.push_str(&format!(" ({ordinal})"));
        }
        if self.documents[i].is_modified() {
            title.push_str(" *");
        }
        title
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
        // Every new document inherits the persisted tab width (ADR 0025); this is
        // the single choke point for the initial document, File ▸ New, and Open.
        doc.editor.set_tab_width(self.settings.tab_width);
        doc.normal = arrange::cascade_slot(self.desktop().size(), self.documents.len(), MIN_WINDOW);
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
            // Hand keyboard focus back to the document being activated — a
            // window-cycle key shouldn't leave keystrokes going to a help
            // window that's no longer topmost (ADR 0027).
            self.defocus_help();
            self.sync_layout();
        }
    }

    /// Activates the next window, wrapping (F6).
    pub fn next_window(&mut self) {
        if self.documents.is_empty() {
            return;
        }
        let n = self.documents.len();
        self.activate((self.active + 1) % n);
    }

    /// Activates the previous window, wrapping (Shift-F6).
    pub fn prev_window(&mut self) {
        if self.documents.is_empty() {
            return;
        }
        let n = self.documents.len();
        self.activate((self.active + n - 1) % n);
    }

    /// Removes the active window. Closing the last one leaves an empty desktop —
    /// New/Open spawn a fresh window again. Otherwise the previous window in
    /// z-order becomes active.
    pub fn remove_active_window(&mut self) {
        if self.documents.is_empty() {
            return;
        }
        self.documents.remove(self.active);
        if self.active >= self.documents.len() {
            self.active = self.documents.len().saturating_sub(1);
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
            doc.normal = arrange::cascade_slot(ds, i, MIN_WINDOW);
        }
        self.sync_layout();
    }

    /// Lays the windows out in a grid that fills the desktop (Window ▸ Tile). Turns
    /// zoom off.
    pub fn tile(&mut self) {
        self.zoomed = false;
        let ds = self.desktop().size();
        let rects = arrange::tile(ds, self.documents.len());
        for (doc, rect) in self.documents.iter_mut().zip(rects) {
            doc.normal = rect;
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

    /// Resolves a status-line hot-key to its command. `StatusLine` is now a pure
    /// display widget (its accelerators are meant to be bound into
    /// `rvision::widgets::Desktop`'s global table, rvision's ADR 0028) — `EditorApp` runs
    /// its own bespoke loop instead of `Desktop` (ADR 0018), so it keeps this
    /// tiny table itself rather than gaining the framework's accelerator
    /// dispatch. Kept in sync with the `StatusItem`s built in [`Self::new`] by
    /// hand; there are only three.
    fn status_key_command(key: &KeyEvent) -> Option<Command> {
        match (key.code, key.modifiers) {
            (KeyCode::F(1), Modifiers::NONE) => Some(CM_HELP),
            (KeyCode::F(2), Modifiers::NONE) => Some(CM_SAVE),
            (KeyCode::Char('x'), Modifiers::ALT) => Some(CM_QUIT),
            _ => None,
        }
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

    /// Routes a mouse event through the local passes. The menu bar gets first
    /// refusal whenever a pull-down is open (it is modal, ADR 0016) or the pointer
    /// is on its row. Otherwise a left-press focuses the window under the pointer and
    /// dispatches by where it landed — close/zoom glyph, a scroll bar, the title bar
    /// (start a move), the resize corner (start a resize), or the interior (drop the
    /// caret). A left-drag continues an active window move/resize, else extends the
    /// editor's selection; releasing ends a drag; the wheel pans the window under the
    /// pointer.
    fn handle_mouse(&mut self, mouse: &MouseEvent, ctx: &mut Context) -> EventResult {
        if (self.menu_bar.is_open() || regions(self.size).menu.contains(mouse.pos))
            && self.menu_bar.handle_event(&Event::Mouse(*mouse), ctx) == EventResult::Consumed
        {
            return EventResult::Consumed;
        }
        match mouse.kind {
            MouseKind::Down(MouseButton::Left) => {
                // The help overlay (ADR 0027) gets first refusal when it's the
                // topmost plane; otherwise documents get it, falling back to help
                // if the press missed every document (help drawn behind them).
                let help_on_top = self.help_focused();
                if help_on_top {
                    if let Some(help_rect) = self.help_rect_screen() {
                        if help_rect.contains(mouse.pos) {
                            return self.handle_help_press(help_rect, mouse, ctx);
                        }
                    }
                }
                let Some(i) = self.window_at(mouse.pos) else {
                    if !help_on_top {
                        if let Some(help_rect) = self.help_rect_screen() {
                            if help_rect.contains(mouse.pos) {
                                return self.handle_help_press(help_rect, mouse, ctx);
                            }
                        }
                    }
                    return EventResult::Ignored;
                };
                // A document press always hands focus back from help, even when
                // the pressed window is already `self.active`.
                self.defocus_help();
                if i != self.active {
                    self.activate(i);
                }
                // A fresh press starts no selection unless it lands in the editor.
                self.selecting = false;
                match self.chrome_hit(self.active, mouse.pos) {
                    ChromeHit::Close => ctx.post(CM_CLOSE), // through the discard guard
                    ChromeHit::Zoom => self.toggle_zoom(),
                    ChromeHit::Move => self.start_move(mouse.pos),
                    ChromeHit::Resize => self.start_resize(mouse.pos),
                    // Unreachable: a Document's ChromeFlags always sets
                    // has_help: false (ADR 0028) — no context-help glyph is
                    // ever drawn for `chrome_hit` to land on.
                    ChromeHit::Help => {}
                    // A scroll bar scrolls (and the thumb starts a drag); otherwise
                    // an interior press places the caret and begins a selection.
                    ChromeHit::None => {
                        if let Some((o, part)) = self.scrollbar_hit(self.active, mouse.pos) {
                            if part == ScrollPart::Thumb {
                                self.drag = Some(Drag::ScrollThumb { orientation: o });
                            } else {
                                self.apply_scroll(self.active, o, part);
                            }
                        } else if inset1(self.window_rect_screen(self.active)).contains(mouse.pos) {
                            self.selecting = true;
                            self.doc_mut()
                                .editor
                                .handle_event(&Event::Mouse(*mouse), ctx);
                        }
                    }
                }
                EventResult::Consumed
            }
            // A drag moves/resizes the window, drags a scroll thumb, or continues
            // a help-window move/resize, if one is under way; otherwise it extends
            // a selection, but only one the editor started.
            MouseKind::Drag(MouseButton::Left) => {
                if self.help.as_ref().is_some_and(|h| h.drag.is_some()) {
                    self.drag_help_to(mouse.pos);
                    EventResult::Consumed
                } else if self.drag.is_some() {
                    self.drag_to(mouse.pos);
                    EventResult::Consumed
                } else if self.selecting {
                    self.doc_mut()
                        .editor
                        .handle_event(&Event::Mouse(*mouse), ctx)
                } else {
                    EventResult::Consumed
                }
            }
            // Releasing ends any window/thumb/help drag or selection (the editor
            // needs no release of its own).
            MouseKind::Up(MouseButton::Left) => {
                let help_was_dragging = self.help.as_mut().and_then(|h| h.drag.take()).is_some();
                let was_active = help_was_dragging || self.drag.take().is_some() || self.selecting;
                self.selecting = false;
                if was_active {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            // The wheel scrolls the window under the pointer without focusing it —
            // help wins ties over an overlapping document, the same simplification
            // draw order already makes (ADR 0027).
            MouseKind::ScrollUp | MouseKind::ScrollDown => {
                if let Some(help_rect) = self.help_rect_screen() {
                    if help_rect.contains(mouse.pos) {
                        return self.forward_to_help(help_rect, mouse, ctx);
                    }
                }
                match self.window_at(mouse.pos) {
                    Some(i) => self.documents[i]
                        .editor
                        .handle_event(&Event::Mouse(*mouse), ctx),
                    None => EventResult::Ignored,
                }
            }
            _ => EventResult::Ignored,
        }
    }

    /// Handles a left press already known to be within the help window's
    /// screen rect: focuses it, then classifies the press exactly like a
    /// document's chrome (close/move/resize) or forwards it into the window's
    /// own interior. Always returns [`EventResult::Consumed`], matching the
    /// document press branch's shape.
    fn handle_help_press(
        &mut self,
        help_rect: Rect,
        mouse: &MouseEvent,
        ctx: &mut Context,
    ) -> EventResult {
        self.focus_help();
        self.selecting = false;
        match Self::chrome_hit_at(help_rect, mouse.pos, false) {
            ChromeHit::Close => ctx.post(CM_CLOSE),
            ChromeHit::Zoom => {} // unreachable: help is built with `.zoomable(false)`
            ChromeHit::Help => {} // unreachable: help's own ChromeFlags sets has_help: false
            ChromeHit::Move => self.start_help_move(mouse.pos),
            ChromeHit::Resize => self.start_help_resize(mouse.pos),
            ChromeHit::None => {
                self.forward_to_help(help_rect, mouse, ctx);
            }
        }
        EventResult::Consumed
    }

    /// Forwards `mouse` (already known to be within `help_rect`) into the help
    /// window, translated to its own local coordinates — mirrors how
    /// `Application::exec_view` translates a mouse event for a modal `Window`.
    fn forward_to_help(
        &mut self,
        help_rect: Rect,
        mouse: &MouseEvent,
        ctx: &mut Context,
    ) -> EventResult {
        let origin = help_rect.origin();
        let local = MouseEvent {
            pos: mouse.pos.offset(-origin.x, -origin.y),
            ..*mouse
        };
        match &mut self.help {
            Some(help) => help.window.handle_event(&Event::Mouse(local), ctx),
            None => EventResult::Ignored,
        }
    }

    /// Routes `event` through the three local passes (menu → editor → status) and
    /// returns the commands those passes posted, for the driver to act on
    /// (ADR 0018). Gated by the live `command_set`: a disabled command's key
    /// or menu selection posts nothing.
    pub fn dispatch(&mut self, event: &Event) -> Vec<Command> {
        let commands = self.command_set();
        let mut ctx = Context::new(&commands);
        match event {
            Event::Key(key) => {
                // menu (pre-process) → window keys → help (if focused) → active
                // editor → status (post).
                let mut result = self.menu_bar.handle_event(event, &mut ctx);
                if result == EventResult::Ignored {
                    result = self.handle_window_key(key, &mut ctx);
                }
                if result == EventResult::Ignored && self.help_focused() {
                    // Esc closes the (non-modal) help window directly — it isn't
                    // built with `.esc_cancels(true)` any more (ADR 0027), so
                    // `Window` itself wouldn't otherwise act on it.
                    if key.code == KeyCode::Esc {
                        self.close_help();
                        result = EventResult::Consumed;
                    } else if let Some(help) = &mut self.help {
                        result = help.window.handle_event(event, &mut ctx);
                    }
                }
                // The active editor never sees a key while help owns the
                // keyboard, even one help's own interior left unhandled.
                if result == EventResult::Ignored
                    && !self.documents.is_empty()
                    && !self.help_focused()
                {
                    result = self.doc_mut().editor.handle_event(event, &mut ctx);
                }
                if result == EventResult::Ignored {
                    if let Some(command) = Self::status_key_command(key) {
                        ctx.post(command);
                    }
                }
            }
            Event::Idle | Event::Broadcast(_) => {
                self.menu_bar.handle_event(event, &mut ctx);
            }
            Event::Mouse(mouse) => {
                self.handle_mouse(mouse, &mut ctx);
            }
            // A bracketed paste goes to the active editor, and also refreshes the
            // internal clipboard so a later Ctrl-V repeats the same external text
            // (least surprise — the two clipboards converge after first contact;
            // ADR 0022).
            Event::Paste(text) => {
                self.clipboard = text.clone();
                if !self.documents.is_empty() {
                    self.doc_mut().editor.handle_event(event, &mut ctx);
                }
            }
            // Resize is handled by the driver (relayout).
            Event::Resize(_) | Event::Command(_) => {}
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
        // With no open window there is no editor to act on; still report the
        // clipboard commands as handled so the driver doesn't fall through.
        if self.documents.is_empty() {
            return matches!(command, CM_COPY | CM_CUT | CM_PASTE);
        }
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

    /// The current clipboard contents. The driver mirrors this to the host system
    /// clipboard after a Cut/Copy (OSC 52, ADR 0021); Paste reads it directly.
    pub fn clipboard(&self) -> &str {
        &self.clipboard
    }

    /// The window draw order, bottom-to-top: inactive windows in z-order, then the
    /// active one on top. A zoomed active window covers the desktop, so only it is
    /// drawn.
    fn draw_order(&self) -> Vec<usize> {
        if self.documents.is_empty() {
            return Vec::new();
        }
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
            // The help overlay (ADR 0027) draws either behind or in front of the
            // whole document stack, depending on which currently has focus — a
            // deliberate two-plane simplification rather than interleaving it
            // into per-document z-order.
            let help_on_top = self.help_focused();
            if let Some(help) = &self.help {
                if !help_on_top {
                    self.draw_help(&mut desk, help);
                }
            }
            for i in self.draw_order() {
                self.draw_window(&mut desk, i);
            }
            if let Some(help) = &self.help {
                if help_on_top {
                    self.draw_help(&mut desk, help);
                }
            }
        }
        self.status_line.draw(&mut canvas.child(r.status));
        self.menu_bar.draw(&mut canvas.child(r.menu));
        // The open pull-down draws last, over everything (ADR 0016).
        self.menu_bar.draw_overlay(canvas);
    }

    /// Draws the help overlay (frame + interior + its own scroll bars, all
    /// handled by `rvision::widgets::Window` itself) at its rectangle within
    /// the desktop canvas `desk` — mirrors [`draw_window`](Self::draw_window)'s
    /// shadow-then-content shape.
    fn draw_help(&self, desk: &mut Canvas, help: &HelpOverlay) {
        let local = help.window.bounds();
        if local.is_empty() {
            return;
        }
        desk.shadow(local, self.shadow_style);
        let mut win = desk.child(local);
        help.window.draw(&mut win);
    }

    /// Draws window `i` (frame + editor + scroll bars) at its effective rectangle
    /// within the desktop canvas `desk`. The active window gets the doubled frame
    /// — but not while the help overlay (ADR 0027) has focus instead.
    fn draw_window(&self, desk: &mut Canvas, i: usize) {
        let local = self.window_rect_local(i);
        if local.is_empty() {
            return;
        }
        // Cast the window's drop shadow on the desktop (or a lower window) first,
        // so this window — drawn next — sits on top of its own shadow (Phase 10).
        desk.shadow(local, self.shadow_style);
        let doc = &self.documents[i];
        let mut win = desk.child(local);
        let area = win.bounds();
        let active = i == self.active && !self.help_focused();
        Frame::new(
            &self.window_title(i),
            self.frame_style,
            if active {
                self.title_style
            } else {
                self.inactive_title_style
            },
        )
        .active(active)
        .maximized(active && self.zoomed)
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
        let m = editor.scroll_metrics();

        // Vertical bar on the right border, between the top and bottom corners.
        // Drawn only when the document overflows the viewport (Phase 10): a bar
        // that can't scroll anywhere just clutters the frame.
        let vbar = vscroll_rect(area.size());
        if m.needs_vertical() && !vbar.is_empty() {
            let mut bar = ScrollBar::new(vbar, self.frame_style);
            bar.set_metrics(m.lines, m.viewport.height.max(0) as usize, m.top);
            bar.draw(&mut win.child(vbar));
        }

        // Horizontal bar on the bottom border, between the left and right corners.
        let hbar = hscroll_rect(area.size());
        if m.needs_horizontal() && !hbar.is_empty() {
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
        self.sync_menu();
        let mut canvas = Canvas::new(frame);
        self.draw_canvas(&mut canvas);
    }

    /// Provided so `EditorApp` is a [`Program`] (the `exec_view` background only
    /// ever calls [`draw`](Self::draw)); the driver uses [`dispatch`](Self::dispatch)
    /// instead, since it needs the posted commands.
    fn handle_event(&mut self, event: &Event) -> EventResult {
        let _ = self.dispatch(event);
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
        for command in ed.dispatch(&event) {
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
    // Clipboard commands need no dialog; act on them first (ADR 0019). Cut/Copy
    // also mirror the internal clipboard to the host via OSC 52 — write-only, so
    // Paste still reads the internal buffer (ADR 0021).
    if ed.handle_clipboard(command) {
        match command {
            CM_COPY | CM_CUT => app.set_clipboard(ed.clipboard())?,
            // An empty internal clipboard on Paste is where users hit the
            // "Ctrl-V didn't grab another app's text" surprise — explain it,
            // since a terminal app cannot read the system clipboard itself
            // (ADR 0021/0022). After any copy or external paste this is rare.
            CM_PASTE if ed.clipboard().is_empty() && ed.window_count() > 0 => message(
                app,
                ed,
                theme,
                "Paste",
                // Prose; MessageBox word-wraps it. Blank line keeps the paragraphs apart.
                "The editor clipboard is empty.\n\n\
                 To paste from another application, use your terminal's paste \
                 (usually Ctrl+Shift+V).",
            )?,
            _ => {}
        }
        return Ok(());
    }
    // On an empty desktop only New/Open, opening a recent file, and Quit do
    // anything; the rest need a document to act on, so they quietly no-op.
    if ed.window_count() == 0
        && !matches!(
            command,
            CM_NEW | CM_OPEN | CM_QUIT | CM_ABOUT | CM_HELP | CM_SETTINGS
        )
        && recent_index(command).is_none()
    {
        return Ok(());
    }
    // A File-menu recent entry opens that file in a new window (ADR 0025).
    if let Some(index) = recent_index(command) {
        return open_recent(app, ed, theme, index);
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
        CM_ABOUT => about(app, ed, theme)?,
        CM_SETTINGS => open_settings(app, ed, theme)?,
        CM_HELP => ed.open_help(theme, None),
        _ => {}
    }
    Ok(())
}

/// Builds the About box's heading: `edit <version>`, with ` (<sha>)` appended
/// when a short git commit hash was stamped in at build time. A blank `sha` — the
/// fallback when git metadata is unavailable (see `build.rs`) — yields just the
/// name and version. Kept pure so it is unit-testable without the build-time
/// stamp (which would otherwise make any snapshot churn every commit).
fn about_version_line(version: &str, sha: &str) -> String {
    let sha = sha.trim();
    if sha.is_empty() {
        format!("edit {version}")
    } else {
        format!("edit {version} ({sha})")
    }
}

/// Shows the About box: name, version (+ git hash when stamped), and the one-line
/// "what this is". A plain `MessageBox::ok`, which word-wraps the prose (blank
/// lines keep the paragraphs apart). The richer Help viewer is still to come.
fn about<T: Backend + EventSource>(
    app: &mut Application<T>,
    ed: &mut EditorApp,
    theme: &Theme,
) -> io::Result<()> {
    let heading = about_version_line(
        env!("CARGO_PKG_VERSION"),
        option_env!("EDIT_GIT_SHA").unwrap_or(""),
    );
    let text = format!(
        "{heading}\n\n\
         A text-mode editor in the spirit of MS-DOS EDIT, built on the \
         rvision TurboVision-style framework.\n\n\
         A Rust learning project."
    );
    message(app, ed, theme, "About", &text)
}

/// Runs the Settings dialog (Edit ▸ Settings), applying the edited values on OK or
/// restoring defaults on "Reset to defaults" (ADR 0025).
fn open_settings<T: Backend + EventSource>(
    app: &mut Application<T>,
    ed: &mut EditorApp,
    theme: &Theme,
) -> io::Result<()> {
    let (mut window, handle) = SettingsDialog::window(theme, ed.settings());
    match app.exec_view(&mut *ed, &mut window)? {
        CM_OK => {
            let dialog = handle.borrow();
            ed.apply_settings_from_dialog(dialog.tab_width(), dialog.recent_limit(), theme)
        }
        CM_DEFAULTS => ed.reset_settings_to_defaults(theme),
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
    let (mut window, handle) = ReplaceDialog::window(
        theme,
        ed.settings().find_case_sensitive,
        ed.settings().find_whole_word,
    );
    if app.exec_view(&mut *ed, &mut window)? != CM_OK {
        return Ok(());
    }
    let (query, replacement) = {
        let dialog = handle.borrow();
        (dialog.query(), dialog.replacement())
    };
    ed.remember_find_options(query.case_sensitive, query.whole_word);
    if query.needle.is_empty() {
        return Ok(());
    }
    let count = ed.active_editor_mut().replace_all(&query, &replacement);
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
    let (mut window, handle) = FindDialog::window(
        theme,
        ed.settings().find_case_sensitive,
        ed.settings().find_whole_word,
    );
    if app.exec_view(&mut *ed, &mut window)? == CM_OK {
        let (query, backward) = {
            let dialog = handle.borrow();
            (dialog.query(), dialog.backward())
        };
        ed.remember_find_options(query.case_sensitive, query.whole_word);
        if !query.needle.is_empty() {
            ed.active_editor_mut().find(query, backward);
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
    let (mut window, handle) = GoToLine::window(theme);
    if app.exec_view(&mut *ed, &mut window)? == CM_OK {
        let line = handle.borrow().line();
        if let Some(line) = line {
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

/// Confirms discarding the active window, then closes it (Window ▸ Close).
/// Closing the last window leaves an empty desktop. A focused help window
/// (ADR 0027) closes directly instead — it has nothing to discard, and isn't
/// the "active window" `remove_active_window` means.
fn close<T: Backend + EventSource>(
    app: &mut Application<T>,
    ed: &mut EditorApp,
    theme: &Theme,
) -> io::Result<()> {
    if ed.help_focused() {
        ed.close_help();
        return Ok(());
    }
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
    let (mut window, result) = FileDialog::open("Open File", ed.start_dir(), theme);
    if app.exec_view(&mut *ed, &mut window)? != CM_OK {
        return Ok(());
    }
    let path = result.path();
    ed.new_window(theme);
    match ed.open_file(path) {
        Ok(lossy) => {
            ed.note_recent(theme);
            if lossy {
                message(
                    app,
                    ed,
                    theme,
                    "Open",
                    "File was not valid UTF-8; loaded lossily.",
                )
            } else {
                Ok(())
            }
        }
        Err(err) => {
            // The load failed: drop the empty window we just opened for it.
            ed.remove_active_window();
            message(app, ed, theme, "Open failed", &err.to_string())
        }
    }
}

/// Opens the `index`-th recent file in a new window (a File-menu recent entry,
/// ADR 0025), like [`open`] but with the path taken from the MRU rather than a
/// dialog. A path that no longer opens is dropped from the list with a note.
fn open_recent<T: Backend + EventSource>(
    app: &mut Application<T>,
    ed: &mut EditorApp,
    theme: &Theme,
    index: usize,
) -> io::Result<()> {
    let Some(path) = ed.recent_path(index) else {
        return Ok(());
    };
    ed.new_window(theme);
    match ed.open_file(path.clone()) {
        Ok(lossy) => {
            ed.note_recent(theme);
            if lossy {
                message(
                    app,
                    ed,
                    theme,
                    "Open",
                    "File was not valid UTF-8; loaded lossily.",
                )
            } else {
                Ok(())
            }
        }
        Err(err) => {
            ed.remove_active_window();
            ed.forget_recent(&path, theme);
            message(
                app,
                ed,
                theme,
                "Open failed",
                &format!("{}\n\n{}", path.display(), err),
            )
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
    let (mut window, result) = FileDialog::save("Save As", ed.start_dir(), theme);
    if app.exec_view(&mut *ed, &mut window)? != CM_OK {
        return Ok(false);
    }
    let path = result.path();
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
        Ok(()) => {
            ed.note_recent(theme);
            Ok(true)
        }
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
        ed.dispatch(&Event::Key(KeyEvent::new(code, mods)))
    }

    #[test]
    fn about_version_line_appends_sha_when_present() {
        assert_eq!(
            about_version_line("0.1.0", "abc1234"),
            "edit 0.1.0 (abc1234)"
        );
    }

    #[test]
    fn about_version_line_omits_sha_when_blank() {
        // build.rs leaves the stamp empty when git metadata is unavailable.
        assert_eq!(about_version_line("0.1.0", ""), "edit 0.1.0");
        assert_eq!(about_version_line("0.1.0", "  "), "edit 0.1.0");
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
        // 'z' matches none of the File menu's hot-keys, so nothing posts — but it
        // is still consumed by the (modal) menu, not leaked to the editor.
        let posted = keydown(&mut ed, KeyCode::Char('z'), Modifiers::NONE);
        assert!(posted.is_empty());
        assert!(!ed.is_modified(), "the keystroke never reached the editor");
    }

    #[test]
    fn hotkey_letters_disambiguate_items_that_share_a_first_letter() {
        // Cut/Copy, Save/Save As..., Find.../Find Next, and Cascade/Close all
        // start with the same letter; build_menu_bar overrides one of each pair
        // so pressing its hot-key routes to the right item, not the first match.
        let mut ed = ed_with_selection();

        keydown(&mut ed, KeyCode::Char('e'), Modifiers::ALT); // open Edit
        let posted = keydown(&mut ed, KeyCode::Char('t'), Modifiers::NONE); // Cut, not Copy
        assert_eq!(posted, vec![CM_CUT]);

        keydown(&mut ed, KeyCode::Char('f'), Modifiers::ALT); // open File
        let posted = keydown(&mut ed, KeyCode::Char('a'), Modifiers::NONE); // Save As, not Save
        assert_eq!(posted, vec![CM_SAVE_AS]);

        keydown(&mut ed, KeyCode::Char('s'), Modifiers::ALT); // open Search
        let posted = keydown(&mut ed, KeyCode::Char('n'), Modifiers::NONE); // Find Next, not Find...
        assert_eq!(posted, vec![CM_FIND_NEXT]);

        keydown(&mut ed, KeyCode::Char('w'), Modifiers::ALT); // open Window
        let posted = keydown(&mut ed, KeyCode::Char('l'), Modifiers::NONE); // Close, not Cascade
        assert_eq!(posted, vec![CM_CLOSE]);
    }

    #[test]
    fn the_file_menu_selects_a_command() {
        let mut ed = app();
        keydown(&mut ed, KeyCode::Char('f'), Modifiers::ALT); // open File (New highlighted)
        let posted = keydown(&mut ed, KeyCode::Enter, Modifiers::NONE);
        assert_eq!(posted, vec![CM_NEW]);
    }

    // --- settings persistence wiring (ADR 0025) ---

    #[test]
    fn recent_command_ids_round_trip_through_recent_index() {
        for i in 0..MAX_RECENT {
            assert_eq!(recent_index(recent_command(i)), Some(i));
        }
        // Neighbours of the block are not recent commands.
        assert_eq!(recent_index(CM_OPEN), None);
        assert_eq!(
            recent_index(Command(CM_RECENT_BASE.0 + MAX_RECENT as u16)),
            None
        );
    }

    #[test]
    fn applied_settings_seed_tab_width_for_existing_and_new_documents() {
        let mut ed = app();
        let mut settings = Settings::default();
        settings.set_tab_width(3);
        ed.apply_settings(settings, &Theme::default());
        // The document that already existed picks up the new width...
        assert_eq!(ed.active_editor().tab_width(), 3);
        // ...and so does a freshly spawned one (the add_document choke point).
        ed.new_window(&Theme::default());
        assert_eq!(ed.active_editor().tab_width(), 3);
    }

    #[test]
    fn applied_recent_files_appear_in_the_file_menu() {
        let mut ed = app();
        let settings = Settings {
            recent: vec![PathBuf::from("/a/alpha.txt"), PathBuf::from("/b/beta.rs")],
            ..Settings::default()
        };
        ed.apply_settings(settings, &Theme::default());

        // File items now read: New, Open, Save, Save As, <recent 0>, <recent 1>, Exit.
        // Selecting the first recent entry posts its CM_RECENT command.
        keydown(&mut ed, KeyCode::Char('f'), Modifiers::ALT); // open File (New highlighted)
        for _ in 0..4 {
            keydown(&mut ed, KeyCode::Down, Modifiers::NONE);
        }
        let posted = keydown(&mut ed, KeyCode::Enter, Modifiers::NONE);
        assert_eq!(posted, vec![recent_command(0)]);
    }

    #[test]
    fn the_recent_files_label_numbers_the_file_name() {
        assert_eq!(
            recent_label(0, Path::new("/home/me/notes.txt")),
            "1 notes.txt"
        );
        assert_eq!(recent_label(2, Path::new("relative.rs")), "3 relative.rs");
    }

    #[test]
    fn the_edit_menu_lists_settings_last() {
        let mut ed = app();
        keydown(&mut ed, KeyCode::Char('e'), Modifiers::ALT); // open Edit (Undo highlighted)
        // Undo, Redo, Cut, Copy, Paste, Settings — five Downs to reach Settings.
        for _ in 0..5 {
            keydown(&mut ed, KeyCode::Down, Modifiers::NONE);
        }
        let posted = keydown(&mut ed, KeyCode::Enter, Modifiers::NONE);
        assert_eq!(posted, vec![CM_SETTINGS]);
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

    // --- system clipboard (OSC 52, ADR 0021) ---

    /// A headless terminal that records what the driver pushes to the host
    /// clipboard, so a test can assert Cut/Copy mirror it and Paste does not.
    #[derive(Default)]
    struct ClipTerminal {
        clipboard: Option<String>,
    }

    impl Backend for ClipTerminal {
        fn size(&self) -> Size {
            Size::new(40, 12)
        }
        fn present(&mut self, _frame: &Buffer) -> io::Result<()> {
            Ok(())
        }
        fn set_clipboard(&mut self, text: &str) -> io::Result<()> {
            self.clipboard = Some(text.to_string());
            Ok(())
        }
    }

    impl EventSource for ClipTerminal {
        fn poll_event(&mut self, _timeout: std::time::Duration) -> io::Result<Option<Event>> {
            Ok(None)
        }
    }

    /// Selects "abc" in a fresh editor, ready for a clipboard command.
    fn ed_with_selection() -> EditorApp {
        let mut ed = app();
        type_chars(&mut ed, "abc");
        run_key(&mut ed, KeyCode::Home, Modifiers::CONTROL);
        run_key(&mut ed, KeyCode::End, Modifiers::SHIFT); // select "abc"
        ed
    }

    #[test]
    fn cut_and_copy_mirror_the_text_to_the_system_clipboard() {
        for command in [CM_COPY, CM_CUT] {
            let mut ed = ed_with_selection();
            let mut sys = Application::new(ClipTerminal::default());
            handle_command(command, &mut sys, &mut ed, &Theme::default()).unwrap();
            assert_eq!(
                sys.terminal().clipboard.as_deref(),
                Some("abc"),
                "{command:?} should push the selection to the host clipboard"
            );
        }
    }

    #[test]
    fn a_bracketed_paste_reaches_the_active_editor_and_mirrors_the_clipboard() {
        // Inbound paste (ADR 0022): the driver routes Event::Paste to the editor…
        let mut ed = app();
        let posted = ed.dispatch(&Event::Paste("pasted\ntext".to_string()));
        assert!(posted.is_empty());
        assert_eq!(ed.active_editor().text(), "pasted\ntext");
        // …and refreshes the internal clipboard, so a later Ctrl-V repeats it.
        assert_eq!(ed.clipboard(), "pasted\ntext");
    }

    #[test]
    fn paste_does_not_touch_the_system_clipboard() {
        // Write-only OSC 52: Paste reads the internal buffer, emits no escape.
        let mut ed = ed_with_selection();
        run_key(&mut ed, KeyCode::Char('c'), Modifiers::CONTROL); // fill internal clipboard
        let mut sys = Application::new(ClipTerminal::default());
        handle_command(CM_PASTE, &mut sys, &mut ed, &Theme::default()).unwrap();
        assert_eq!(sys.terminal().clipboard, None);
    }

    // --- disabled (greyed) menu items (Phase 4 backlog) ---

    #[test]
    fn command_set_disables_undo_redo_cut_copy_and_save_until_there_is_something_to_act_on() {
        let ed = app(); // fresh, untouched document
        let cs = ed.command_set();
        for command in [CM_UNDO, CM_REDO, CM_CUT, CM_COPY, CM_SAVE] {
            assert!(!cs.is_enabled(command), "{command:?} should start disabled");
        }
        // These need only an open document, not any particular state.
        for command in [CM_SAVE_AS, CM_FIND, CM_FIND_NEXT, CM_REPLACE, CM_GOTO] {
            assert!(cs.is_enabled(command), "{command:?} needs no special state");
        }

        let mut ed = ed_with_selection(); // typed "abc", then selected it
        let cs = ed.command_set();
        assert!(cs.is_enabled(CM_UNDO), "typing left something to undo");
        assert!(cs.is_enabled(CM_CUT), "the selection makes Cut/Copy live");
        assert!(cs.is_enabled(CM_COPY));
        assert!(cs.is_enabled(CM_SAVE), "typing modified the document");
        assert!(!cs.is_enabled(CM_REDO), "nothing has been undone yet");

        ed.active_editor_mut().undo();
        assert!(
            ed.command_set().is_enabled(CM_REDO),
            "the undo left something to redo"
        );
    }

    #[test]
    fn command_set_disables_everything_but_the_empty_desktop_allowlist_with_no_window_open() {
        let mut ed = app();
        ed.remove_active_window();
        let cs = ed.command_set();
        for command in [
            CM_SAVE,
            CM_SAVE_AS,
            CM_UNDO,
            CM_REDO,
            CM_CUT,
            CM_COPY,
            CM_PASTE,
            CM_FIND,
            CM_FIND_NEXT,
            CM_REPLACE,
            CM_GOTO,
            CM_CLOSE,
            CM_NEXT_WINDOW,
            CM_PREV_WINDOW,
            CM_CASCADE,
            CM_TILE,
            CM_ZOOM,
        ] {
            assert!(!cs.is_enabled(command), "{command:?} needs a document");
        }
        // Mirrors the empty-desktop allowlist in `handle_command`.
        for command in [CM_NEW, CM_OPEN, CM_QUIT, CM_ABOUT, CM_HELP, CM_SETTINGS] {
            assert!(
                cs.is_enabled(command),
                "{command:?} works on an empty desktop"
            );
        }
    }

    #[test]
    fn ctrl_v_still_posts_paste_on_an_empty_clipboard_though_the_menu_greys_it() {
        // Paste is the one command where the menu's grey state and dispatch's gate
        // deliberately disagree: greyed in the pull-down, but Ctrl-V still posts
        // CM_PASTE so handle_command's "clipboard is empty" explainer still fires.
        let mut ed = app();
        assert!(ed.clipboard().is_empty());
        assert!(ed.command_set().is_enabled(CM_PASTE));
        let posted = keydown(&mut ed, KeyCode::Char('v'), Modifiers::CONTROL);
        assert_eq!(posted, vec![CM_PASTE]);
    }

    #[test]
    fn disabled_edit_items_grey_in_the_running_menu() {
        // The framework half (rvision's MenuBar::sync_enabled) was proven in
        // isolation; this is the editor half actually reaching it, so a disabled
        // item is visible as grey in the running app, not just in a unit test.
        let theme = THEME();
        let disabled = theme.style(Role::MenuDisabled);

        let mut ed = app(); // fresh: no undo, no selection, empty clipboard, unmodified
        let width = ed.size.width;
        let row_is_greyed = |buf: &Buffer, row: i16| {
            (0..width).any(|x| buf.get(Point::new(x, row)).unwrap().style() == disabled)
        };

        keydown(&mut ed, KeyCode::Char('e'), Modifiers::ALT); // open Edit (Undo highlighted)
        let mut buf = Buffer::new(ed.size);
        ed.draw(&mut buf); // Program::draw syncs the live CommandSet in first

        // Undo(row 2)/Redo(3)/Cut(4)/Copy(5): nothing to undo/redo/select yet.
        for row in [2, 3, 4, 5] {
            assert!(row_is_greyed(&buf, row), "row {row} should be greyed");
        }
        // Paste(6) greys too on an empty clipboard (dispatch still gates it
        // differently — see the Ctrl-V test above).
        assert!(row_is_greyed(&buf, 6), "Paste should be greyed");
        // Settings...(7) needs no state, so it stays live.
        assert!(!row_is_greyed(&buf, 7), "Settings should not be greyed");

        keydown(&mut ed, KeyCode::Esc, Modifiers::NONE); // close without posting
        type_chars(&mut ed, "abc");
        run_key(&mut ed, KeyCode::Home, Modifiers::CONTROL);
        run_key(&mut ed, KeyCode::End, Modifiers::SHIFT); // select "abc"
        keydown(&mut ed, KeyCode::Char('e'), Modifiers::ALT); // reopen Edit
        let mut buf = Buffer::new(ed.size);
        ed.draw(&mut buf);

        for row in [2, 4, 5] {
            assert!(
                !row_is_greyed(&buf, row),
                "row {row} should no longer be greyed"
            );
        }
        assert!(
            row_is_greyed(&buf, 6),
            "Paste is still greyed: the clipboard is still empty"
        );
    }

    #[test]
    fn the_edit_menu_posts_its_commands() {
        // A fresh, untouched document disables Undo and Cut (nothing to undo, no
        // selection — see `command_set_disables_...` below), so give it typed,
        // selected text to select from.
        let mut ed = ed_with_selection();
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

    #[test]
    fn start_dir_of_a_fresh_untitled_document_is_the_current_dir() {
        let ed = app();
        assert_eq!(ed.start_dir(), std::env::current_dir().unwrap());
    }

    #[test]
    fn start_dir_of_a_bare_filename_with_no_directory_is_the_current_dir() {
        // `Path::parent()` returns `Some("")`, not `None`, for a single-component
        // relative path — the `edit somename.txt` command-line case (main.rs) must
        // not mistake that empty parent for a real directory to list.
        let mut ed = app();
        ed.open_or_new("a-file-that-does-not-exist.txt").unwrap();
        assert_eq!(ed.start_dir(), std::env::current_dir().unwrap());
    }

    #[test]
    fn start_dir_of_a_path_with_a_real_parent_is_that_parent() {
        let mut ed = app();
        ed.open_or_new("/tmp/some-edit-test-subdir/whatever.txt")
            .unwrap();
        assert_eq!(ed.start_dir(), PathBuf::from("/tmp/some-edit-test-subdir"));
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
    fn the_help_menu_posts_help_topics_then_about() {
        let mut ed = app();
        keydown(&mut ed, KeyCode::Char('h'), Modifiers::ALT); // open Help (Help Topics highlighted)
        assert!(ed.menu_is_open());
        // First item: Help Topics.
        assert_eq!(
            keydown(&mut ed, KeyCode::Enter, Modifiers::NONE),
            vec![CM_HELP]
        );
        // Second item: About.
        keydown(&mut ed, KeyCode::Char('h'), Modifiers::ALT);
        keydown(&mut ed, KeyCode::Down, Modifiers::NONE);
        assert_eq!(
            keydown(&mut ed, KeyCode::Enter, Modifiers::NONE),
            vec![CM_ABOUT]
        );
    }

    #[test]
    fn f1_opens_help_via_the_status_line() {
        let mut ed = app();
        // F1 is declined by the editor and claimed by the post-process status line.
        assert_eq!(
            keydown(&mut ed, KeyCode::F(1), Modifiers::NONE),
            vec![CM_HELP]
        );
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
    fn closing_the_last_window_leaves_an_empty_desktop() {
        let mut ed = app();
        type_chars(&mut ed, "stuff");
        ed.remove_active_window();
        assert_eq!(ed.window_count(), 0, "the desktop can be emptied");
        // New opens a fresh window again.
        ed.new_window(&THEME());
        assert_eq!(ed.window_count(), 1);
        assert_eq!(ed.active_editor().text(), "");
    }

    #[test]
    fn an_empty_desktop_ignores_keys_but_still_draws_and_exits() {
        let mut ed = app();
        ed.remove_active_window();
        assert_eq!(ed.window_count(), 0);
        // Typing reaches no editor and posts nothing — no panic on the empty desktop.
        assert!(keydown(&mut ed, KeyCode::Char('h'), Modifiers::NONE).is_empty());
        // Drawing the empty desktop composes without panicking.
        let mut buf = Buffer::new(Size::new(40, 12));
        ed.draw_canvas(&mut Canvas::new(&mut buf));
        // Window-management commands quietly no-op rather than panic.
        ed.next_window();
        ed.prev_window();
        ed.toggle_zoom();
        // Alt-X still posts Quit via the status line.
        assert_eq!(
            keydown(&mut ed, KeyCode::Char('x'), Modifiers::ALT),
            vec![CM_QUIT]
        );
    }

    #[test]
    fn an_open_menu_swallows_window_keys() {
        let mut ed = app();
        ed.new_window(&THEME()); // index 1, active
        keydown(&mut ed, KeyCode::Char('f'), Modifiers::ALT); // open File
        keydown(&mut ed, KeyCode::F(6), Modifiers::NONE); // should be swallowed by the menu
        assert_eq!(ed.active_index(), 1, "F6 never reached window switching");
    }

    // --- MDI: window titles / instance indicators ---

    #[test]
    fn a_lone_window_keeps_its_plain_title() {
        let ed = app();
        assert_eq!(ed.window_title(0), "Untitled");
    }

    #[test]
    fn windows_sharing_a_title_get_instance_indicators() {
        let mut ed = app();
        ed.new_window(&THEME());
        ed.new_window(&THEME()); // three Untitled
        assert_eq!(ed.window_title(0), "Untitled (1)");
        assert_eq!(ed.window_title(1), "Untitled (2)");
        assert_eq!(ed.window_title(2), "Untitled (3)");
    }

    #[test]
    fn distinct_names_are_not_numbered() {
        let mut ed = app();
        ed.new_window(&THEME());
        ed.documents[0].path = Some(PathBuf::from("/x/a.txt"));
        assert_eq!(
            ed.window_title(0),
            "a.txt",
            "unique name needs no indicator"
        );
        assert_eq!(ed.window_title(1), "Untitled");
    }

    #[test]
    fn the_instance_indicator_precedes_the_modified_star() {
        let mut ed = app();
        ed.new_window(&THEME());
        type_chars(&mut ed, "x"); // modifies the active window (index 1)
        assert_eq!(ed.window_title(0), "Untitled (1)");
        assert_eq!(ed.window_title(1), "Untitled (2) *");
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

    #[test]
    fn scrollbars_appear_only_when_the_document_overflows() {
        let render = |ed: &EditorApp| {
            let mut buf = Buffer::new(Size::new(40, 12));
            ed.draw_canvas(&mut Canvas::new(&mut buf));
            buf.to_text()
        };

        let mut ed = EditorApp::new(Size::new(40, 12), &THEME());
        // A document that fits the viewport draws no scroll bars (Phase 10).
        ed.active_editor_mut().set_text("short");
        let text = render(&ed);
        assert!(!text.contains('▲'), "no vertical bar when the text fits");
        assert!(!text.contains('◄'), "no horizontal bar when the text fits");

        // More lines than the viewport brings the vertical bar back.
        ed.active_editor_mut().set_text(&"line\n".repeat(40));
        assert!(
            render(&ed).contains('▲'),
            "a vertical bar appears once the text overflows"
        );
    }

    #[test]
    fn an_overlapping_window_casts_a_drop_shadow_on_the_desktop() {
        let mut ed = EditorApp::new(Size::new(40, 12), &THEME());
        // Un-zoom and place a small window away from the desktop edges, so its
        // shadow falls on the backdrop rather than being clipped off-screen.
        ed.zoomed = false;
        ed.documents[0].normal = Rect::from_origin_size(Point::new(2, 2), Size::new(10, 4));
        ed.sync_layout();

        let mut buf = Buffer::new(Size::new(40, 12));
        ed.draw_canvas(&mut Canvas::new(&mut buf));

        // The window spans desktop-local (2,2)..(12,6); the desktop starts at screen
        // row 1, so the right-edge shadow lands at screen (12, 5).
        assert_eq!(
            buf.get(Point::new(12, 5)).unwrap().style(),
            THEME().style(Role::Shadow)
        );
        // Backdrop clear of the window and its shadow is left alone.
        assert_eq!(
            buf.get(Point::new(0, 1)).unwrap().style(),
            THEME().style(Role::DesktopBackground)
        );
    }

    // --- mouse: click-to-focus (Phase 9a) ---

    fn left_click(ed: &mut EditorApp, x: i16, y: i16) -> Vec<Command> {
        ed.dispatch(&Event::Mouse(MouseEvent {
            kind: MouseKind::Down(MouseButton::Left),
            pos: Point::new(x, y),
            modifiers: Modifiers::NONE,
        }))
    }

    #[test]
    fn clicking_an_inactive_window_makes_it_active() {
        let mut ed = EditorApp::new(Size::new(40, 12), &THEME());
        ed.new_window(&THEME()); // active = 1
        ed.tile(); // window 0 = left half, window 1 = right half, un-zoomed
        assert_eq!(ed.active_index(), 1);
        left_click(&mut ed, 5, 5); // inside window 0 (left half of the desktop)
        assert_eq!(ed.active_index(), 0);
    }

    #[test]
    fn clicking_the_active_window_keeps_it_active() {
        let mut ed = EditorApp::new(Size::new(40, 12), &THEME());
        ed.new_window(&THEME());
        ed.tile();
        left_click(&mut ed, 25, 5); // inside window 1 (right half), already active
        assert_eq!(ed.active_index(), 1);
    }

    #[test]
    fn clicking_outside_the_desktop_changes_nothing() {
        let mut ed = EditorApp::new(Size::new(40, 12), &THEME());
        ed.new_window(&THEME());
        ed.tile();
        let before = ed.active_index();
        left_click(&mut ed, 5, 0); // a bare stretch of the menu-bar row
        assert_eq!(ed.active_index(), before);
    }

    // --- mouse: menu bar (Phase 9b) ---

    #[test]
    fn clicking_a_menu_title_opens_the_pulldown() {
        let mut ed = app();
        assert!(!ed.menu_is_open());
        left_click(&mut ed, 1, 0); // the File title
        assert!(ed.menu_is_open());
    }

    #[test]
    fn clicking_a_menu_item_posts_its_command() {
        let mut ed = app();
        left_click(&mut ed, 1, 0); // open File (New is the first item)
        let posted = left_click(&mut ed, 3, 3); // click an item row
        assert!(!ed.menu_is_open());
        assert!(!posted.is_empty(), "choosing an item posts its command");
    }

    #[test]
    fn clicking_the_editor_while_a_menu_is_open_dismisses_it() {
        let mut ed = app();
        left_click(&mut ed, 1, 0); // open File
        assert!(ed.menu_is_open());
        left_click(&mut ed, 5, 5); // click down in the editor
        assert!(!ed.menu_is_open(), "the click-away closes the menu");
    }

    // --- mouse: editor interior + wheel (Phase 9c) ---

    fn mouse_at(ed: &mut EditorApp, kind: MouseKind, x: i16, y: i16) -> Vec<Command> {
        ed.dispatch(&Event::Mouse(MouseEvent {
            kind,
            pos: Point::new(x, y),
            modifiers: Modifiers::NONE,
        }))
    }

    #[test]
    fn clicking_in_a_window_interior_places_the_caret() {
        let mut ed = app(); // one zoomed window; interior starts at screen (1, 2)
        ed.active_editor_mut().set_text("hello world");
        left_click(&mut ed, 4, 2); // editor-local column 3 on the first line
        assert_eq!(ed.active_editor().cursor().line, 0);
        assert_eq!(ed.active_editor().cursor().column, 3);
    }

    #[test]
    fn clicking_the_vertical_bar_down_arrow_scrolls_a_line() {
        let mut ed = app(); // zoomed window over a (40, 10) desktop
        ed.active_editor_mut().set_text(&"line\n".repeat(30));
        assert_eq!(ed.active_editor().scroll_metrics().top, 0);
        left_click(&mut ed, 39, 9); // the ▼ at the foot of the right-hand bar
        assert_eq!(ed.active_editor().scroll_metrics().top, 1);
    }

    #[test]
    fn clicking_the_vertical_bar_track_below_the_thumb_pages_down() {
        let mut ed = app();
        ed.active_editor_mut().set_text(&"line\n".repeat(30));
        left_click(&mut ed, 39, 6); // track between the thumb and the ▼
        assert_eq!(
            ed.active_editor().scroll_metrics().top,
            8, // one viewport (the interior is 8 rows tall)
        );
    }

    #[test]
    fn clicking_the_horizontal_bar_right_arrow_scrolls_a_column() {
        let mut ed = app();
        ed.active_editor_mut().set_text(&"x".repeat(100));
        assert_eq!(ed.active_editor().scroll_metrics().left, 0);
        left_click(&mut ed, 38, 10); // the ► at the end of the bottom bar
        assert_eq!(ed.active_editor().scroll_metrics().left, 1);
    }

    #[test]
    fn dragging_the_vertical_thumb_scrolls_without_selecting() {
        let mut ed = app(); // zoomed window; the right-hand bar runs screen rows 2..10
        ed.active_editor_mut().set_text(&"line\n".repeat(30));
        assert_eq!(ed.active_editor().scroll_metrics().top, 0);
        left_click(&mut ed, 39, 3); // press the thumb (sitting at the top of the track)
        drag_to(&mut ed, 39, 8); // drag it down the track
        assert!(
            ed.active_editor().scroll_metrics().top > 0,
            "the thumb drag panned the view down"
        );
        assert_eq!(
            ed.active_editor().selected_text(),
            None,
            "dragging the thumb must not select text"
        );
        release(&mut ed, 39, 8);
    }

    #[test]
    fn a_drag_that_began_on_a_scroll_bar_never_selects_text() {
        let mut ed = app();
        ed.active_editor_mut().set_text(&"line\n".repeat(30));
        left_click(&mut ed, 39, 9); // the ▼ arrow — a scroll, not an interior press
        drag_to(&mut ed, 20, 5); // drag on into the interior
        assert_eq!(
            ed.active_editor().selected_text(),
            None,
            "a drag begun off the text never leaks into a selection"
        );
    }

    #[test]
    fn the_wheel_scrolls_the_window_under_the_pointer() {
        let mut ed = app();
        ed.active_editor_mut().set_text(&"line\n".repeat(20));
        assert_eq!(ed.active_editor().scroll_metrics().top, 0);
        mouse_at(&mut ed, MouseKind::ScrollDown, 5, 5);
        assert!(
            ed.active_editor().scroll_metrics().top > 0,
            "the wheel pans the view down"
        );
    }

    // --- mouse: window chrome drag (Phase 9d) ---

    fn drag_to(ed: &mut EditorApp, x: i16, y: i16) {
        mouse_at(ed, MouseKind::Drag(MouseButton::Left), x, y);
    }

    fn release(ed: &mut EditorApp, x: i16, y: i16) {
        mouse_at(ed, MouseKind::Up(MouseButton::Left), x, y);
    }

    #[test]
    fn clicking_the_close_glyph_posts_close() {
        let mut ed = app(); // one zoomed window over a 40-wide desktop
        let posted = left_click(&mut ed, 3, 1); // the [■] glyph on the top edge
        assert_eq!(posted, vec![CM_CLOSE]); // routed through the driver's guard
    }

    #[test]
    fn clicking_the_zoom_glyph_toggles_zoom() {
        let mut ed = app();
        assert!(ed.zoomed);
        left_click(&mut ed, 37, 1); // the [↑] glyph near the top-right
        assert!(!ed.zoomed);
    }

    #[test]
    fn grabbing_the_title_of_a_maximised_window_unzooms_it() {
        let mut ed = app();
        assert!(ed.zoomed);
        left_click(&mut ed, 10, 1); // the title bar
        assert!(!ed.zoomed, "starting a drag un-maximises");
    }

    #[test]
    fn dragging_the_title_bar_moves_the_window() {
        let mut ed = EditorApp::new(Size::new(40, 12), &THEME());
        ed.new_window(&THEME());
        ed.tile(); // window 1 active at desktop-local (20, 0), screen origin (20, 1)
        assert_eq!(ed.documents[1].normal.origin().x, 20);
        left_click(&mut ed, 25, 1); // grab the title bar (offset 5 from the left)
        drag_to(&mut ed, 5, 3); // drag left
        release(&mut ed, 5, 3);
        assert_eq!(ed.documents[1].normal.origin().x, 0);
        assert!(ed.drag.is_none(), "the release ends the drag");
    }

    #[test]
    fn dragging_the_corner_resizes_the_window() {
        let mut ed = EditorApp::new(Size::new(40, 12), &THEME());
        ed.new_window(&THEME());
        ed.tile(); // window 1: screen (20, 1) size (20, 10), corner cell (39, 10)
        left_click(&mut ed, 39, 10); // grab the resize corner
        drag_to(&mut ed, 30, 6);
        release(&mut ed, 30, 6);
        let r = ed.documents[1].normal;
        assert!(
            r.width() < 20 && r.height() < 10,
            "the window shrank to {:?}",
            r.size()
        );
    }

    // --- help overlay: a standalone, non-modal resident window (ADR 0027) ---

    #[test]
    fn open_help_creates_a_centred_overlay_and_focuses_it() {
        let mut ed = EditorApp::new(Size::new(80, 24), &THEME());
        assert!(ed.help.is_none());
        ed.open_help(&THEME(), None);
        assert!(ed.help_focused());
        let r = ed.help.as_ref().unwrap().window.bounds();
        let ds = ed.desktop().size();
        assert!(!r.is_empty());
        assert!(r.width() <= ds.width && r.height() <= ds.height);
        // Centred, not pinned to the top-left corner.
        assert!(r.origin().x > 0 && r.origin().y > 0);
    }

    #[test]
    fn open_help_again_while_open_just_refocuses_without_rebuilding() {
        let mut ed = EditorApp::new(Size::new(80, 24), &THEME());
        ed.open_help(&THEME(), None);
        let original = ed.help.as_ref().unwrap().window.bounds();
        ed.defocus_help();
        assert!(!ed.help_focused());
        ed.open_help(&THEME(), None);
        assert!(ed.help_focused());
        assert_eq!(
            ed.help.as_ref().unwrap().window.bounds(),
            original,
            "reused the existing window rather than rebuilding it"
        );
    }

    #[test]
    fn clicking_a_document_defocuses_an_open_help_window() {
        let mut ed = EditorApp::new(Size::new(80, 24), &THEME());
        ed.new_window(&THEME());
        ed.tile(); // window 0 = left half, window 1 = right half, both un-zoomed
        ed.open_help(&THEME(), None);
        assert!(ed.help_focused());
        let help_rect = ed.help_rect_screen().unwrap();
        // The tiled left window's own top-left corner — far from help's centred rect.
        let doc_point = Point::new(2, 2);
        assert!(
            !help_rect.contains(doc_point),
            "test setup: point must miss help, which was at {help_rect:?}"
        );
        left_click(&mut ed, doc_point.x, doc_point.y);
        assert!(!ed.help_focused());
    }

    #[test]
    fn clicking_the_help_window_on_an_empty_desktop_focuses_it() {
        let mut ed = EditorApp::new(Size::new(80, 24), &THEME());
        ed.remove_active_window();
        assert_eq!(ed.window_count(), 0);
        ed.open_help(&THEME(), None);
        ed.defocus_help();
        assert!(!ed.help_focused());
        let r = ed.help_rect_screen().unwrap();
        let inside = Point::new(r.origin().x + r.width() / 2, r.origin().y + r.height() / 2);
        left_click(&mut ed, inside.x, inside.y);
        assert!(
            ed.help_focused(),
            "a click on the (unfocused) help window brings it back"
        );
    }

    #[test]
    fn clicking_the_help_close_glyph_posts_close() {
        let mut ed = EditorApp::new(Size::new(80, 24), &THEME());
        ed.open_help(&THEME(), None);
        let r = ed.help_rect_screen().unwrap();
        let close_col = Frame::close_span(r.width(), false).unwrap().start;
        let posted = left_click(&mut ed, r.origin().x + close_col, r.origin().y);
        assert_eq!(posted, vec![CM_CLOSE]);
    }

    #[test]
    fn zoom_glyph_is_never_hit_for_help() {
        // With `zoomable: false`, `Frame` never draws a zoom glyph there at all
        // (confirmed by the snapshot below), so the column it would otherwise
        // occupy is just ordinary title-bar territory — a move, not a dead
        // zone. The property this actually guards is that `ChromeHit::Zoom`
        // itself is unreachable, not that the column does nothing.
        let r = Rect::from_origin_size(Point::new(0, 0), Size::new(20, 10));
        let zoom_col = Frame::zoom_span(r.width(), false).unwrap().start;
        let hit = EditorApp::chrome_hit_at(r, Point::new(zoom_col, 0), false);
        assert!(!matches!(hit, ChromeHit::Zoom));
    }

    #[test]
    fn esc_closes_a_focused_help_window() {
        let mut ed = EditorApp::new(Size::new(80, 24), &THEME());
        ed.open_help(&THEME(), None);
        assert!(ed.help_focused());
        keydown(&mut ed, KeyCode::Esc, Modifiers::NONE);
        assert!(ed.help.is_none());
    }

    #[test]
    fn activating_a_window_defocuses_help() {
        let mut ed = EditorApp::new(Size::new(80, 24), &THEME());
        ed.new_window(&THEME());
        ed.open_help(&THEME(), None);
        assert!(ed.help_focused());
        ed.activate(0);
        assert!(!ed.help_focused());
    }

    #[test]
    fn dragging_the_help_title_bar_moves_it() {
        let mut ed = EditorApp::new(Size::new(80, 24), &THEME());
        ed.open_help(&THEME(), None);
        let before = ed.help.as_ref().unwrap().window.bounds();
        let r = ed.help_rect_screen().unwrap();
        let grab = Point::new(r.origin().x + r.width() / 2, r.origin().y); // mid title bar
        left_click(&mut ed, grab.x, grab.y);
        drag_to(&mut ed, grab.x - 5, grab.y + 3);
        release(&mut ed, grab.x - 5, grab.y + 3);
        let after = ed.help.as_ref().unwrap().window.bounds();
        assert_ne!(
            after.origin(),
            before.origin(),
            "the title-bar drag moved the window"
        );
        assert_eq!(after.size(), before.size(), "a move must not resize");
        assert!(
            ed.help.as_ref().unwrap().drag.is_none(),
            "the release ends the drag"
        );
    }

    #[test]
    fn dragging_the_help_corner_resizes_it() {
        let mut ed = EditorApp::new(Size::new(80, 24), &THEME());
        ed.open_help(&THEME(), None);
        let r = ed.help_rect_screen().unwrap();
        let corner = r.bottom_right().offset(-1, -1);
        left_click(&mut ed, corner.x, corner.y);
        drag_to(&mut ed, corner.x - 10, corner.y - 5);
        release(&mut ed, corner.x - 10, corner.y - 5);
        let after = ed.help.as_ref().unwrap().window.bounds();
        assert!(
            after.width() < r.width() && after.height() < r.height(),
            "the corner drag shrank the window to {:?}",
            after.size()
        );
    }

    #[test]
    fn relayout_clamps_an_out_of_bounds_help_window() {
        let mut ed = EditorApp::new(Size::new(80, 24), &THEME());
        ed.open_help(&THEME(), None);
        ed.relayout(Size::new(20, 10)); // much smaller terminal
        let r = ed.help.as_ref().unwrap().window.bounds();
        let ds = ed.desktop().size();
        assert!(r.width() <= ds.width && r.height() <= ds.height);
        assert!(r.origin().x + r.width() <= ds.width);
        assert!(r.origin().y + r.height() <= ds.height);
    }

    /// A backend whose `poll_event` fails immediately — used to prove
    /// `close()` never reaches `confirm_discard`'s `exec_view` prompt when a
    /// focused help window (not a document) is what's being closed. If it did,
    /// this test would error out (or hang, against a backend that returns
    /// `Ok(None)` forever) instead of passing.
    struct NoPollBackend;

    impl Backend for NoPollBackend {
        fn size(&self) -> Size {
            Size::new(80, 24)
        }
        fn present(&mut self, _frame: &Buffer) -> io::Result<()> {
            Ok(())
        }
    }

    impl EventSource for NoPollBackend {
        fn poll_event(&mut self, _timeout: std::time::Duration) -> io::Result<Option<Event>> {
            Err(io::Error::other(
                "close() must not prompt when help (not a document) is focused",
            ))
        }
    }

    #[test]
    fn close_command_closes_a_focused_help_window_without_the_discard_guard() {
        let mut ed = EditorApp::new(Size::new(80, 24), &THEME());
        type_chars(&mut ed, "unsaved"); // dirty the document — would normally prompt
        assert!(ed.is_modified());
        ed.open_help(&THEME(), None);
        assert!(ed.help_focused());
        let mut sys = Application::new(NoPollBackend);
        close(&mut sys, &mut ed, &THEME()).unwrap();
        assert!(ed.help.is_none(), "help closed");
        assert_eq!(ed.window_count(), 1, "the document was untouched");
        assert!(
            ed.is_modified(),
            "the document's unsaved changes are untouched"
        );
    }

    #[test]
    fn snapshot_help_window_open_and_focused_over_a_document() {
        let mut ed = EditorApp::new(Size::new(60, 20), &THEME());
        ed.active_editor_mut().set_text("hello world");
        ed.open_help(&THEME(), None);
        let mut buf = Buffer::new(Size::new(60, 20));
        ed.draw_canvas(&mut Canvas::new(&mut buf));
        insta::assert_snapshot!(buf.to_text());
    }
}
