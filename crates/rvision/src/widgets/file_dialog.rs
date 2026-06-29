//! A file Open/Save dialog (TurboVision's `TFileDialog`).
//!
//! Composes a [`ListBox`](super::ListBox) of directory entries, an
//! [`InputLine`](super::InputLine) for the file name, and *Open*/*Save* +
//! *Cancel* [`Button`](super::Button)s into a modal view (ADR 0017). `Enter` on a
//! directory navigates into it; `Enter` on a file (or the default button) accepts
//! the path; `Esc` cancels. After [`exec_view`](crate::app::Application::exec_view)
//! returns `CM_OK`, [`path`](FileDialog::path) is the chosen file.
//!
//! Directory listing is read through an injected closure (real `std::fs` by
//! default), so navigation is testable without touching the filesystem.

use std::fs;
use std::path::{Path, PathBuf};

use crate::canvas::Canvas;
use crate::cell::Cell;
use crate::color::Style;
use crate::command::{CM_CANCEL, CM_OK, Command};
use crate::event::{Event, EventResult, KeyCode, KeyEvent, MouseButton, MouseEvent, MouseKind};
use crate::geometry::{Point, Rect, Size};
use crate::theme::{Role, Theme};
use crate::view::{Context, Modal, View};

use super::{Button, InputLine, ListBox};

/// One directory entry: a name and whether it is a sub-directory.
#[derive(Clone)]
struct Entry {
    name: String,
    is_dir: bool,
}

/// How a [`FileDialog`] lists a directory's own entries (no `..`; the dialog adds
/// that itself). The default is [`read_dir_entries`]; tests inject a fake.
type Reader = Box<dyn Fn(&Path) -> Vec<Entry>>;

const FOCUS_LIST: usize = 0;
const FOCUS_INPUT: usize = 1;
const FOCUS_OPEN: usize = 2;
const FOCUS_CANCEL: usize = 3;
const FOCUS_COUNT: usize = 4;

/// A modal file picker.
pub struct FileDialog {
    size: Size,
    title: String,
    style: Style,
    theme: Theme,
    reader: Reader,
    dir: PathBuf,
    entries: Vec<Entry>,
    list: ListBox,
    list_rect: Rect,
    input: InputLine,
    open: Button,
    cancel: Button,
    focus: usize,
}

impl FileDialog {
    /// An *Open* dialog titled `title`, starting in `dir`.
    pub fn open(title: &str, dir: impl Into<PathBuf>, theme: &Theme) -> Self {
        Self::with_reader(title, dir.into(), "Open", theme, Box::new(read_dir_entries))
    }

    /// A *Save* dialog titled `title`, starting in `dir`.
    pub fn save(title: &str, dir: impl Into<PathBuf>, theme: &Theme) -> Self {
        Self::with_reader(title, dir.into(), "Save", theme, Box::new(read_dir_entries))
    }

    /// The shared constructor (the `reader` seam makes navigation testable).
    fn with_reader(title: &str, dir: PathBuf, accept: &str, theme: &Theme, reader: Reader) -> Self {
        let size = Size::new(46, 18);
        let iw = size.width - 2;
        let ih = size.height - 2;
        let list_rect = rect(0, 4, iw, ih - 6);

        let mut dialog = Self {
            size,
            title: title.to_string(),
            style: theme.style(Role::DialogBackground),
            theme: theme.clone(),
            reader,
            dir: PathBuf::new(),
            entries: Vec::new(),
            list: ListBox::new(list_rect, Vec::new(), theme),
            list_rect,
            input: InputLine::new(rect(0, 1, iw, 1), theme),
            open: Button::new(rect(iw - 24, ih - 1, 10, 1), accept, CM_OK, theme).default(true),
            cancel: Button::new(rect(iw - 12, ih - 1, 10, 1), "Cancel", CM_CANCEL, theme),
            focus: FOCUS_LIST,
        };
        dialog.set_dir(dir);
        dialog
    }

    /// The path the dialog currently points at: the directory joined with the
    /// name field. Read this after `exec_view` returns `CM_OK`.
    pub fn path(&self) -> PathBuf {
        self.dir.join(self.input.text())
    }

    /// Reads `dir`, rebuilds the list (with a leading `..` unless at the root),
    /// clears the name field, and returns focus to the list.
    fn set_dir(&mut self, dir: PathBuf) {
        let mut entries = (self.reader)(&dir);
        if dir.parent().is_some() {
            entries.insert(
                0,
                Entry {
                    name: "..".to_string(),
                    is_dir: true,
                },
            );
        }
        let display: Vec<String> = entries.iter().map(display_of).collect();
        self.list = ListBox::new(self.list_rect, display, &self.theme);
        self.entries = entries;
        self.dir = dir;
        self.input.set_text("");
        self.focus = FOCUS_LIST;
        self.apply_focus();
    }

    /// Pushes the focus flag to whichever control now holds it (ADR 0017).
    fn apply_focus(&mut self) {
        self.list.set_focused(self.focus == FOCUS_LIST);
        self.input.set_focused(self.focus == FOCUS_INPUT);
        self.open.set_focused(self.focus == FOCUS_OPEN);
        self.cancel.set_focused(self.focus == FOCUS_CANCEL);
    }

    /// Moves focus `delta` steps round the four controls.
    fn move_focus(&mut self, delta: isize) {
        let n = FOCUS_COUNT as isize;
        self.focus = (((self.focus as isize + delta) % n + n) % n) as usize;
        self.apply_focus();
    }

    /// Navigates into entry `index` (a directory or `..`).
    fn navigate(&mut self, index: usize) {
        let entry = &self.entries[index];
        let new_dir = if entry.name == ".." {
            self.dir
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.dir.clone())
        } else {
            self.dir.join(&entry.name)
        };
        self.set_dir(new_dir);
    }

    /// Mirrors the highlighted list entry into the name field (blank for a
    /// directory), the way a real file picker does.
    fn sync_input_from_list(&mut self) {
        if let Some(i) = self.list.selected() {
            let entry = &self.entries[i];
            let text = if entry.is_dir { "" } else { &entry.name };
            self.input.set_text(text);
        }
    }

    /// `Enter`: navigate into a directory, accept a file/typed path, or
    /// activate the focused button.
    fn on_enter(&mut self, ctx: &mut Context) -> EventResult {
        match self.focus {
            FOCUS_LIST => match self.list.selected() {
                Some(i) if self.entries[i].is_dir => {
                    self.navigate(i);
                    EventResult::Consumed
                }
                Some(i) => {
                    self.input.set_text(&self.entries[i].name);
                    ctx.post(CM_OK);
                    EventResult::Consumed
                }
                None => EventResult::Ignored,
            },
            FOCUS_CANCEL => {
                ctx.post(CM_CANCEL);
                EventResult::Consumed
            }
            // The name field or the Open/Save button: accept the typed path.
            _ => {
                ctx.post(CM_OK);
                EventResult::Consumed
            }
        }
    }

    /// Routes a non-navigation key to the focused control.
    fn route(&mut self, key: KeyEvent, ctx: &mut Context) -> EventResult {
        let event = Event::Key(key);
        match self.focus {
            FOCUS_LIST => {
                let result = self.list.handle_event(&event, ctx);
                self.sync_input_from_list();
                result
            }
            FOCUS_INPUT => self.input.handle_event(&event, ctx),
            FOCUS_OPEN => self.open.handle_event(&event, ctx),
            FOCUS_CANCEL => self.cancel.handle_event(&event, ctx),
            _ => EventResult::Ignored,
        }
    }

    /// The interior rectangle (inset one cell on every side) in local coordinates.
    fn interior(&self) -> Rect {
        Rect::from_origin_size(
            Point::new(1, 1),
            Size::new((self.size.width - 2).max(0), (self.size.height - 2).max(0)),
        )
    }

    /// Routes a mouse event (in dialog-local coordinates) to the control under the
    /// pointer, focusing it on a left-press. Control bounds are interior-local, so
    /// the pointer is shifted by the interior origin, then into the control's own
    /// coordinates. Clicking the list mirrors arrow navigation by syncing the name
    /// field to the new selection.
    fn handle_mouse(&mut self, m: &MouseEvent, ctx: &mut Context) -> EventResult {
        let io = self.interior().origin();
        let p = m.pos.offset(-io.x, -io.y);
        let bounds = [
            self.list.bounds(),
            self.input.bounds(),
            self.open.bounds(),
            self.cancel.bounds(),
        ];
        let Some(i) = bounds.iter().position(|b| b.contains(p)) else {
            return EventResult::Ignored;
        };
        if matches!(m.kind, MouseKind::Down(MouseButton::Left)) {
            self.focus = i;
            self.apply_focus();
        }
        let b = bounds[i];
        let local = Event::Mouse(MouseEvent {
            pos: p.offset(-b.origin().x, -b.origin().y),
            ..*m
        });
        match i {
            FOCUS_LIST => {
                let result = self.list.handle_event(&local, ctx);
                self.sync_input_from_list();
                result
            }
            FOCUS_INPUT => self.input.handle_event(&local, ctx),
            FOCUS_OPEN => self.open.handle_event(&local, ctx),
            FOCUS_CANCEL => self.cancel.handle_event(&local, ctx),
            _ => EventResult::Ignored,
        }
    }
}

/// How an entry shows in the list: `..`, a directory `name/`, or a file `name`.
fn display_of(entry: &Entry) -> String {
    if entry.name == ".." {
        "..".to_string()
    } else if entry.is_dir {
        format!("{}/", entry.name)
    } else {
        entry.name.clone()
    }
}

/// Reads a directory's own entries from the real filesystem — sub-directories
/// first, then files, each group sorted by name. Unreadable directories list
/// empty rather than erroring.
fn read_dir_entries(path: &Path) -> Vec<Entry> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    if let Ok(read) = fs::read_dir(path) {
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                dirs.push(Entry { name, is_dir });
            } else {
                files.push(Entry { name, is_dir });
            }
        }
    }
    dirs.sort_by(|a, b| a.name.cmp(&b.name));
    files.sort_by(|a, b| a.name.cmp(&b.name));
    dirs.into_iter().chain(files).collect()
}

fn rect(x: i16, y: i16, w: i16, h: i16) -> Rect {
    Rect::from_origin_size(Point::new(x, y), Size::new(w, h))
}

impl View for FileDialog {
    fn bounds(&self) -> Rect {
        Rect::from_origin_size(Point::new(0, 0), self.size)
    }

    fn draw(&self, canvas: &mut Canvas) {
        let area = canvas.bounds();
        canvas.fill(area, &Cell::blank(self.style));
        canvas.draw_box(area, self.style);
        if !self.title.is_empty() && area.width() > 4 {
            let label = format!(" {} ", self.title);
            let len = label.chars().count() as i16;
            let x = ((area.width() - len) / 2).max(1);
            canvas.put_str(Point::new(x, 0), &label, self.style);
        }

        let interior = self.interior();
        if interior.is_empty() {
            return;
        }
        let mut sub = canvas.child(interior);
        sub.put_str(Point::new(0, 0), "Name:", self.style);
        sub.put_str(Point::new(0, 3), "Files:", self.style);
        for control in [
            &self.input as &dyn View,
            &self.list,
            &self.open,
            &self.cancel,
        ] {
            let mut child = sub.child(control.bounds());
            control.draw(&mut child);
        }
    }

    fn handle_event(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        let key = match event {
            Event::Key(key) => key,
            Event::Mouse(m) => return self.handle_mouse(m, ctx),
            _ => return EventResult::Ignored,
        };
        match key.code {
            KeyCode::Esc => {
                ctx.post(CM_CANCEL);
                EventResult::Consumed
            }
            KeyCode::Tab => {
                self.move_focus(1);
                EventResult::Consumed
            }
            KeyCode::BackTab => {
                self.move_focus(-1);
                EventResult::Consumed
            }
            KeyCode::Enter => self.on_enter(ctx),
            _ => self.route(*key, ctx),
        }
    }

    fn focusable(&self) -> bool {
        true
    }

    fn drop_shadow(&self) -> Option<Style> {
        // A modal always floats over the background, so it always casts (ADR 0020).
        Some(self.theme.style(Role::Shadow))
    }
}

impl Modal for FileDialog {
    fn size(&self) -> Size {
        self.size
    }

    fn ends_on(&self, command: Command) -> bool {
        command == CM_OK || command == CM_CANCEL
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::command::CommandSet;
    use crate::event::Modifiers;

    fn entry(name: &str, is_dir: bool) -> Entry {
        Entry {
            name: name.to_string(),
            is_dir,
        }
    }

    /// A fake filesystem: `/root` holds a sub-dir and two files; `/root/sub`
    /// holds one file.
    fn fake_reader() -> Reader {
        Box::new(|path: &Path| match path.to_str().unwrap_or_default() {
            "/root" => vec![
                entry("sub", true),
                entry("a.txt", false),
                entry("b.txt", false),
            ],
            "/root/sub" => vec![entry("c.txt", false)],
            _ => vec![],
        })
    }

    fn dialog() -> FileDialog {
        FileDialog::with_reader(
            "Open",
            PathBuf::from("/root"),
            "Open",
            &Theme::default(),
            fake_reader(),
        )
    }

    fn press(d: &mut FileDialog, code: KeyCode) -> EventResult {
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        d.handle_event(&Event::Key(KeyEvent::new(code, Modifiers::NONE)), &mut ctx)
    }

    fn names(d: &FileDialog) -> Vec<String> {
        d.entries.iter().map(|e| e.name.clone()).collect()
    }

    #[test]
    fn lists_directories_first_with_a_parent_entry() {
        let d = dialog();
        assert_eq!(names(&d), vec!["..", "sub", "a.txt", "b.txt"]);
        assert!(d.entries[1].is_dir, "sub is a directory");
        assert!(!d.entries[2].is_dir, "a.txt is a file");
    }

    #[test]
    fn clicking_a_file_in_the_list_selects_it_and_fills_the_name() {
        let mut d = dialog(); // entries: .., sub, a.txt, b.txt
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        // The list starts at dialog-local (1, 5); "a.txt" is its row 2 → y = 7.
        let click = Event::Mouse(MouseEvent {
            kind: MouseKind::Down(MouseButton::Left),
            pos: Point::new(3, 7),
            modifiers: Modifiers::NONE,
        });
        assert_eq!(d.handle_event(&click, &mut ctx), EventResult::Consumed);
        assert_eq!(d.focus, FOCUS_LIST);
        assert_eq!(d.list.selected(), Some(2));
        assert_eq!(d.input.text(), "a.txt", "the name field follows the click");
    }

    #[test]
    fn enter_on_a_directory_navigates_into_it() {
        let mut d = dialog();
        press(&mut d, KeyCode::Down); // select "sub"
        press(&mut d, KeyCode::Enter);
        assert_eq!(d.dir, PathBuf::from("/root/sub"));
        assert_eq!(names(&d), vec!["..", "c.txt"]);
    }

    #[test]
    fn dotdot_navigates_to_the_parent() {
        let mut d = dialog();
        press(&mut d, KeyCode::Down);
        press(&mut d, KeyCode::Enter); // into /root/sub
        assert_eq!(d.dir, PathBuf::from("/root/sub"));
        // ".." is the first entry; Enter on it goes back up.
        press(&mut d, KeyCode::Enter);
        assert_eq!(d.dir, PathBuf::from("/root"));
    }

    #[test]
    fn enter_on_a_file_accepts_with_its_path() {
        let mut d = dialog();
        press(&mut d, KeyCode::Down); // sub
        press(&mut d, KeyCode::Down); // a.txt
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        let r = d.handle_event(
            &Event::Key(KeyEvent::new(KeyCode::Enter, Modifiers::NONE)),
            &mut ctx,
        );
        assert_eq!(r, EventResult::Consumed);
        assert_eq!(ctx.posted(), &[Event::Command(CM_OK)]);
        assert_eq!(d.path(), PathBuf::from("/root/a.txt"));
    }

    #[test]
    fn typing_in_the_name_field_builds_the_path() {
        let mut d = dialog();
        press(&mut d, KeyCode::Tab); // focus the name field
        assert_eq!(d.focus, FOCUS_INPUT);
        for c in "new.txt".chars() {
            press(&mut d, KeyCode::Char(c));
        }
        assert_eq!(d.path(), PathBuf::from("/root/new.txt"));
    }

    #[test]
    fn tab_cycles_focus_through_the_four_controls() {
        let mut d = dialog();
        assert_eq!(d.focus, FOCUS_LIST);
        press(&mut d, KeyCode::Tab);
        assert_eq!(d.focus, FOCUS_INPUT);
        press(&mut d, KeyCode::Tab);
        assert_eq!(d.focus, FOCUS_OPEN);
        press(&mut d, KeyCode::Tab);
        assert_eq!(d.focus, FOCUS_CANCEL);
        press(&mut d, KeyCode::Tab);
        assert_eq!(d.focus, FOCUS_LIST, "wraps");
        press(&mut d, KeyCode::BackTab);
        assert_eq!(d.focus, FOCUS_CANCEL, "and back the other way");
    }

    #[test]
    fn esc_cancels() {
        let mut d = dialog();
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        d.handle_event(
            &Event::Key(KeyEvent::new(KeyCode::Esc, Modifiers::NONE)),
            &mut ctx,
        );
        assert_eq!(ctx.posted(), &[Event::Command(CM_CANCEL)]);
    }

    #[test]
    fn ends_on_ok_and_cancel_only() {
        let d = dialog();
        assert!(Modal::ends_on(&d, CM_OK));
        assert!(Modal::ends_on(&d, CM_CANCEL));
        assert!(!Modal::ends_on(&d, Command(crate::command::CM_USER + 1)));
    }

    #[test]
    fn snapshot_file_dialog() {
        let d = dialog();
        let mut buf = Buffer::new(d.size());
        let mut canvas = Canvas::new(&mut buf);
        d.draw(&mut canvas);
        insta::assert_snapshot!(buf.to_text());
    }
}
