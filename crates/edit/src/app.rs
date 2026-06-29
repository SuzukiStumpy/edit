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

use crate::editor::{CM_COPY, CM_CUT, CM_PASTE, CM_REDO, CM_UNDO, EditorView};
use crate::file::{self, Encoding};

/// File ▸ New.
pub const CM_NEW: Command = Command(CM_USER + 1);
/// File ▸ Open.
pub const CM_OPEN: Command = Command(CM_USER + 2);
/// File ▸ Save.
pub const CM_SAVE: Command = Command(CM_USER + 3);
/// File ▸ Save As.
pub const CM_SAVE_AS: Command = Command(CM_USER + 4);

/// The editor application root: chrome + the owned document view + the current
/// file's path and [`Encoding`].
pub struct EditorApp {
    menu_bar: MenuBar,
    status_line: StatusLine,
    editor: EditorView,
    size: Size,
    /// The open file's path, or `None` for an unsaved new document.
    path: Option<PathBuf>,
    /// The encoding to preserve on save (ADR 0010).
    encoding: Encoding,
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
                        MenuItem::new("Open...", CM_OPEN).with_shortcut("F3"),
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
                StatusItem::new(
                    "F3",
                    "Open",
                    KeyEvent::new(KeyCode::F(3), Modifiers::NONE),
                    CM_OPEN,
                ),
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
        let mut editor = EditorView::new(Rect::default(), theme);
        editor.set_focused(true);

        let mut app = Self {
            menu_bar,
            status_line,
            editor,
            size,
            path: None,
            encoding: Encoding::new_file(),
            clipboard: String::new(),
            finished: false,
            frame_style: theme.style(Role::WindowFrame),
            title_style: theme.style(Role::WindowTitle),
            backdrop: Cell::from_char('░', theme.style(Role::DesktopBackground)),
        };
        app.relayout(size);
        app
    }

    /// Repositions the chrome and the editor for a terminal of `size`.
    pub fn relayout(&mut self, size: Size) {
        self.size = size;
        let r = regions(size);
        self.menu_bar.set_bounds(r.menu);
        self.status_line.set_bounds(r.status);
        self.editor.set_bounds(inset1(r.desktop));
    }

    /// Whether a menu pull-down is open (for tests / the loop).
    pub fn menu_is_open(&self) -> bool {
        self.menu_bar.is_open()
    }

    /// Whether the document has unsaved changes.
    pub fn is_modified(&self) -> bool {
        self.editor.is_modified()
    }

    /// Whether the loop should stop.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// The open file's path, if any.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
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

    /// The directory a file dialog should start in: the current file's folder, or
    /// the process working directory.
    pub fn start_dir(&self) -> PathBuf {
        self.path
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    // --- file operations (terminal-free, unit-tested) ---

    /// Resets to an empty, unsaved document.
    pub fn new_file(&mut self) {
        self.editor.set_text("");
        self.path = None;
        self.encoding = Encoding::new_file();
    }

    /// Loads `path` into the editor, adopting its encoding. Returns whether the
    /// bytes were decoded lossily (so the caller can warn).
    ///
    /// # Errors
    ///
    /// Propagates any I/O error from reading the file.
    pub fn open_file(&mut self, path: impl Into<PathBuf>) -> io::Result<bool> {
        let path = path.into();
        let loaded = file::load(&path)?;
        self.editor.set_text(&loaded.text);
        self.encoding = loaded.encoding;
        self.path = Some(path);
        Ok(loaded.lossy)
    }

    /// Writes the document to `path` (adopting it as the current file) using the
    /// preserved encoding, then clears the dirty flag.
    ///
    /// # Errors
    ///
    /// Propagates any I/O error from writing the file.
    pub fn save_to(&mut self, path: impl Into<PathBuf>) -> io::Result<()> {
        let path = path.into();
        file::save(&path, &self.editor.text(), &self.encoding)?;
        self.editor.mark_saved();
        self.path = Some(path);
        Ok(())
    }

    /// Opens `path` if it exists, otherwise starts an empty document that will be
    /// created at `path` on the first save (the `edit FILE` command-line case).
    ///
    /// # Errors
    ///
    /// Propagates any I/O error from reading an existing file.
    pub fn open_or_new(&mut self, path: impl Into<PathBuf>) -> io::Result<bool> {
        let path = path.into();
        if path.exists() {
            self.open_file(path)
        } else {
            self.editor.set_text("");
            self.encoding = Encoding::new_file();
            self.path = Some(path);
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
            Event::Key(_) => {
                self.menu_bar
                    .handle_event(event, &mut ctx)
                    .or_else(|| self.editor.handle_event(event, &mut ctx))
                    .or_else(|| self.status_line.handle_event(event, &mut ctx));
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
                if let Some(text) = self.editor.selected_text() {
                    self.clipboard = text;
                }
                true
            }
            CM_CUT => {
                if let Some(text) = self.editor.take_selection() {
                    self.clipboard = text;
                }
                true
            }
            CM_PASTE => {
                if !self.clipboard.is_empty() {
                    let text = self.clipboard.clone();
                    self.editor.insert_text(&text);
                }
                true
            }
            _ => false,
        }
    }

    /// Draws the whole screen into `canvas`.
    fn draw_canvas(&self, canvas: &mut Canvas) {
        let r = regions(self.size);
        {
            let mut win = canvas.child(r.desktop);
            let area = win.bounds();
            win.fill(area, &self.backdrop);
            Frame::new(&self.title(), self.frame_style, self.title_style)
                .active(true)
                .draw(&mut win);
            let interior = inset1(area);
            if !interior.is_empty() {
                self.editor.draw(&mut win.child(interior));
                self.draw_scrollbars(&mut win, area);
            }
        }
        self.status_line.draw(&mut canvas.child(r.status));
        self.menu_bar.draw(&mut canvas.child(r.menu));
        // The open pull-down draws last, over everything (ADR 0016).
        self.menu_bar.draw_overlay(canvas);
    }

    /// Draws the vertical and horizontal scroll bars over the window's right and
    /// bottom border, reflecting the editor's position in the document. `win` is
    /// the window's canvas and `area` its local bounds (`(0, 0)`-origin).
    fn draw_scrollbars(&self, win: &mut Canvas, area: Rect) {
        let Size { width, height } = area.size();
        let m = self.editor.scroll_metrics();

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
            ed.editor.undo();
        }
        CM_REDO => {
            ed.editor.redo();
        }
        CM_QUIT => {
            if confirm_discard(app, ed, theme)? {
                ed.finished = true;
            }
        }
        CM_NEW => {
            if confirm_discard(app, ed, theme)? {
                ed.new_file();
            }
        }
        CM_OPEN => open(app, ed, theme)?,
        CM_SAVE => {
            save(app, ed, theme)?;
        }
        CM_SAVE_AS => {
            save_as(app, ed, theme)?;
        }
        _ => {}
    }
    Ok(())
}

/// Offers to save unsaved changes before a discarding action. Returns whether it
/// is OK to proceed (saved, or the user chose to discard); `false` cancels.
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

/// Runs the Open dialog and loads the chosen file.
fn open<T: Backend + EventSource>(
    app: &mut Application<T>,
    ed: &mut EditorApp,
    theme: &Theme,
) -> io::Result<()> {
    if !confirm_discard(app, ed, theme)? {
        return Ok(());
    }
    let mut dialog = FileDialog::open("Open File", ed.start_dir(), theme);
    if app.exec_view(&mut *ed, &mut dialog)? != CM_OK {
        return Ok(());
    }
    let path = dialog.path();
    match ed.open_file(path) {
        Ok(true) => message(
            app,
            ed,
            theme,
            "Open",
            "File was not valid UTF-8; loaded lossily.",
        ),
        Ok(false) => Ok(()),
        Err(err) => message(app, ed, theme, "Open failed", &err.to_string()),
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
        assert_eq!(ed.editor.text(), "h");
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
        assert_eq!(ed.editor.text(), "abcabc");
    }

    #[test]
    fn cut_removes_the_selection_and_paste_restores_it() {
        let mut ed = app();
        type_chars(&mut ed, "abc");
        run_key(&mut ed, KeyCode::Home, Modifiers::CONTROL);
        run_key(&mut ed, KeyCode::End, Modifiers::SHIFT); // select "abc"
        run_key(&mut ed, KeyCode::Char('x'), Modifiers::CONTROL); // cut
        assert_eq!(ed.editor.text(), "");
        run_key(&mut ed, KeyCode::Char('v'), Modifiers::CONTROL); // paste it back
        assert_eq!(ed.editor.text(), "abc");
    }

    #[test]
    fn paste_with_an_empty_clipboard_is_a_no_op() {
        let mut ed = app();
        type_chars(&mut ed, "x");
        assert!(ed.handle_clipboard(CM_PASTE));
        assert_eq!(ed.editor.text(), "x");
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
        assert_eq!(ed.editor.text(), "abc");
        keydown(&mut ed, KeyCode::Char('z'), Modifiers::CONTROL); // editor handles it
        assert_eq!(ed.editor.text(), "", "the typing run undoes as one unit");
        assert!(!ed.is_modified(), "undone back to the empty saved state");
    }

    #[test]
    fn the_undo_menu_command_routes_to_the_editor() {
        let mut ed = app();
        type_chars(&mut ed, "z");
        assert!(ed.editor.undo(), "a pending action to undo");
        assert!(ed.editor.redo(), "and to redo");
        assert_eq!(ed.editor.text(), "z");
    }

    #[test]
    fn scrollbars_draw_on_the_window_frame_and_track_the_position() {
        use rvision::buffer::Buffer;
        use rvision::canvas::Canvas;

        let mut ed = EditorApp::new(Size::new(20, 10), &Theme::default());
        // 20 lines, each wider than the interior (18): both axes overflow.
        let line = "abcdefghijklmnopqrstuvwxyz";
        ed.editor.set_text(
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
            ed.editor.bounds(),
            inset1(regions(Size::new(30, 10)).desktop)
        );
    }

    #[test]
    fn new_file_clears_the_document_and_path() {
        let mut ed = app();
        keydown(&mut ed, KeyCode::Char('z'), Modifiers::NONE);
        ed.path = Some(PathBuf::from("/tmp/whatever.txt"));
        ed.new_file();
        assert_eq!(ed.editor.text(), "");
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
        assert_eq!(ed.editor.text(), "one\ntwo\n");
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
}
