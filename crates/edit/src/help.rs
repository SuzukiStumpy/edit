//! The editor's help: the baked-in content and a modal two-pane viewer.
//!
//! The viewer is an editor concept (like the `dialogs` modals): it composes the
//! generic `rvision` help parts — the [`HelpContents`](rvision::help::HelpContents)
//! model and the [`HelpPane`](rvision::widgets::HelpPane) page renderer — with a
//! [`ListBox`](rvision::widgets::ListBox) of topic titles, run modally via
//! [`exec_view`](rvision::app::Application::exec_view) (ADR 0017/0018). The
//! framework's own (desktop-window) viewer is deferred until the windowing
//! question is settled (see the roadmap Backlog); this modal needs none of that.
//!
//! Content lives in `help.txt`, compiled into the binary with `include_str!`
//! (ADR 0023). A future authoring app would emit the same format.

use rvision::canvas::Canvas;
use rvision::cell::Cell;
use rvision::color::Style;
use rvision::command::{CM_CANCEL, CM_OK, Command};
use rvision::event::{Event, EventResult, KeyCode, MouseButton, MouseEvent, MouseKind};
use rvision::geometry::{Point, Rect, Size};
use rvision::help::HelpContents;
use rvision::theme::{Role, Theme};
use rvision::view::{Context, Modal, View};
use rvision::widgets::{HelpPane, ListBox};

/// The editor's help content, baked into the binary (ADR 0023).
pub const HELP_TEXT: &str = include_str!("help.txt");

const FOCUS_LIST: usize = 0;
const FOCUS_PAGE: usize = 1;

/// Layout: the contents list is this wide; a one-column separator follows, then
/// the page pane fills the rest. The bottom interior row is a key hint.
const LIST_W: i16 = 20;

/// A modal help browser: a contents list on the left, a scrollable page on the
/// right. Moving the list selection updates the page live; `Tab` switches panes;
/// `Esc` closes.
pub struct HelpViewer {
    size: Size,
    style: Style,
    frame_style: Style,
    shadow_style: Style,
    contents: HelpContents,
    list: ListBox,
    pane: HelpPane,
    focus: usize,
}

impl HelpViewer {
    /// Builds the viewer over `source` (the help markup), opening at topic
    /// `initial` if given and known, else the home topic.
    pub fn new(source: &str, initial: Option<&str>, theme: &Theme) -> Self {
        let contents = HelpContents::parse(source);
        let size = Size::new(72, 20);
        let (interior_w, interior_h) = (size.width - 2, size.height - 2);
        let content_h = interior_h - 1; // bottom row is the hint
        let titles: Vec<String> = contents.titles().iter().map(|s| s.to_string()).collect();

        let mut list = ListBox::new(local_rect(0, 0, LIST_W, content_h), titles, theme);
        let pane = HelpPane::new(
            local_rect(LIST_W + 1, 0, interior_w - (LIST_W + 1), content_h),
            theme,
        );
        if let Some(id) = initial {
            if let Some(i) = contents.topics().iter().position(|t| t.id == id) {
                list.select(i);
            }
        }

        let mut viewer = Self {
            size,
            style: theme.style(Role::DialogBackground),
            frame_style: theme.style(Role::WindowFrame),
            shadow_style: theme.style(Role::Shadow),
            contents,
            list,
            pane,
            focus: FOCUS_LIST,
        };
        viewer.apply_focus();
        viewer.sync_page();
        viewer
    }

    /// Shows the currently-selected topic in the page pane.
    fn sync_page(&mut self) {
        if let Some(i) = self.list.selected() {
            if let Some(topic) = self.contents.topics().get(i) {
                self.pane.show(topic);
            }
        }
    }

    /// Pushes the focus flag to whichever pane now holds it (ADR 0017).
    fn apply_focus(&mut self) {
        self.list.set_focused(self.focus == FOCUS_LIST);
        self.pane.set_focused(self.focus == FOCUS_PAGE);
    }

    /// The interior rectangle (inset one cell on every side), dialog-local.
    fn interior(&self) -> Rect {
        local_rect(1, 1, self.size.width - 2, self.size.height - 2)
    }

    /// Routes a key to the focused pane; live-updates the page when the list
    /// selection moves.
    fn route_key(&mut self, event: &Event, ctx: &mut Context) -> EventResult {
        if self.focus == FOCUS_LIST {
            let before = self.list.selected();
            let result = self.list.handle_event(event, ctx);
            if self.list.selected() != before {
                self.sync_page();
            }
            result
        } else {
            self.pane.handle_event(event, ctx)
        }
    }

    /// Routes a mouse event (dialog-local) to the pane under the pointer. A left
    /// press also moves focus there; the wheel/scroll-bar act without stealing
    /// focus. Live-updates the page on a list click.
    fn handle_mouse(&mut self, m: &MouseEvent, ctx: &mut Context) -> EventResult {
        let io = self.interior().origin();
        let p = m.pos.offset(-io.x, -io.y);
        let is_press = matches!(m.kind, MouseKind::Down(MouseButton::Left));
        let target = if self.list.bounds().contains(p) {
            FOCUS_LIST
        } else if self.pane.bounds().contains(p) {
            FOCUS_PAGE
        } else {
            return EventResult::Ignored;
        };
        if is_press {
            self.focus = target;
            self.apply_focus();
        }
        let bounds = if target == FOCUS_LIST {
            self.list.bounds()
        } else {
            self.pane.bounds()
        };
        let local = Event::Mouse(MouseEvent {
            pos: p.offset(-bounds.origin().x, -bounds.origin().y),
            ..*m
        });
        if target == FOCUS_LIST {
            let before = self.list.selected();
            let result = self.list.handle_event(&local, ctx);
            if self.list.selected() != before {
                self.sync_page();
            }
            result
        } else {
            self.pane.handle_event(&local, ctx)
        }
    }
}

/// A dialog/interior-local rectangle.
fn local_rect(x: i16, y: i16, w: i16, h: i16) -> Rect {
    Rect::from_origin_size(Point::new(x, y), Size::new(w.max(0), h.max(0)))
}

impl View for HelpViewer {
    fn bounds(&self) -> Rect {
        Rect::from_origin_size(Point::new(0, 0), self.size)
    }

    fn drop_shadow(&self) -> Option<Style> {
        Some(self.shadow_style) // the classic TV shadow, painted by the compositor (ADR 0020)
    }

    fn draw(&self, canvas: &mut Canvas) {
        let area = canvas.bounds();
        canvas.fill(area, &Cell::blank(self.style));
        canvas.draw_box(area, self.style);
        let title = " Help ";
        let x = ((area.width() - title.chars().count() as i16) / 2).max(1);
        canvas.put_str(Point::new(x, 0), title, self.style);

        let interior = self.interior();
        if interior.is_empty() {
            return;
        }
        let mut sub = canvas.child(interior);

        {
            let mut list_canvas = sub.child(self.list.bounds());
            self.list.draw(&mut list_canvas);
        }

        // The vertical separator between the panes.
        let content_h = self.list.bounds().height();
        for y in 0..content_h {
            sub.set(
                Point::new(LIST_W, y),
                Cell::from_char('│', self.frame_style),
            );
        }

        {
            let mut pane_canvas = sub.child(self.pane.bounds());
            self.pane.draw(&mut pane_canvas);
        }

        let hint = "↑↓ Topic   Tab Switch pane   Esc Close";
        sub.put_str(Point::new(0, content_h), hint, self.style);
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
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = 1 - self.focus;
                self.apply_focus();
                EventResult::Consumed
            }
            KeyCode::Enter if self.focus == FOCUS_LIST => {
                self.focus = FOCUS_PAGE;
                self.apply_focus();
                EventResult::Consumed
            }
            _ => self.route_key(event, ctx),
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}

impl Modal for HelpViewer {
    fn size(&self) -> Size {
        self.size
    }

    fn ends_on(&self, command: Command) -> bool {
        command == CM_CANCEL || command == CM_OK
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rvision::command::CommandSet;
    use rvision::event::{KeyEvent, Modifiers};

    const SRC: &str = "\
@topic overview  Overview
Welcome to edit.

@topic keyboard  Keyboard
Ctrl+S saves.

@topic clipboard  Clipboard
Ctrl+V pastes the editor clipboard.";

    fn viewer(initial: Option<&str>) -> HelpViewer {
        HelpViewer::new(SRC, initial, &Theme::default())
    }

    fn press(v: &mut HelpViewer, code: KeyCode) -> Vec<Event> {
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        v.handle_event(&Event::Key(KeyEvent::new(code, Modifiers::NONE)), &mut ctx);
        ctx.take_posted()
    }

    /// Renders the page pane region to text, to assert which topic is shown.
    fn page_text(v: &HelpViewer) -> String {
        use rvision::buffer::Buffer;
        let mut buf = Buffer::new(v.size);
        let mut canvas = Canvas::new(&mut buf);
        v.draw(&mut canvas);
        buf.to_text()
    }

    #[test]
    fn snapshot_help_viewer() {
        use rvision::buffer::Buffer;
        let v = viewer(None);
        let mut buf = Buffer::new(v.size);
        let mut canvas = Canvas::new(&mut buf);
        v.draw(&mut canvas);
        insta::assert_snapshot!(buf.to_text());
    }

    #[test]
    fn opens_on_the_home_topic_by_default() {
        let v = viewer(None);
        assert_eq!(v.list.selected(), Some(0));
        assert!(page_text(&v).contains("Welcome to edit."));
    }

    #[test]
    fn opens_on_an_initial_topic_when_given() {
        let v = viewer(Some("clipboard"));
        assert_eq!(v.list.selected(), Some(2));
        assert!(page_text(&v).contains("editor clipboard"));
    }

    #[test]
    fn an_unknown_initial_topic_falls_back_to_home() {
        let v = viewer(Some("nope"));
        assert_eq!(v.list.selected(), Some(0));
    }

    #[test]
    fn arrowing_the_list_live_updates_the_page() {
        let mut v = viewer(None);
        assert!(page_text(&v).contains("Welcome to edit."));
        press(&mut v, KeyCode::Down); // → Keyboard
        assert_eq!(v.list.selected(), Some(1));
        assert!(page_text(&v).contains("Ctrl+S saves."));
        assert!(!page_text(&v).contains("Welcome to edit."));
    }

    #[test]
    fn tab_switches_focus_between_the_panes() {
        let mut v = viewer(None);
        assert_eq!(v.focus, FOCUS_LIST);
        press(&mut v, KeyCode::Tab);
        assert_eq!(v.focus, FOCUS_PAGE);
        // With focus on the page, Down scrolls the page, not the list selection.
        let before = v.list.selected();
        press(&mut v, KeyCode::Down);
        assert_eq!(
            v.list.selected(),
            before,
            "list selection unchanged on the page"
        );
        press(&mut v, KeyCode::BackTab);
        assert_eq!(v.focus, FOCUS_LIST);
    }

    #[test]
    fn enter_on_the_list_jumps_into_the_page() {
        let mut v = viewer(None);
        press(&mut v, KeyCode::Enter);
        assert_eq!(v.focus, FOCUS_PAGE);
    }

    #[test]
    fn esc_ends_the_modal() {
        let mut v = viewer(None);
        assert_eq!(press(&mut v, KeyCode::Esc), vec![Event::Command(CM_CANCEL)]);
        assert!(v.ends_on(CM_CANCEL));
    }

    // --- the shipped content (a compile-in safety net, ADR 0023) ---

    fn topic_text(t: &rvision::help::HelpTopic) -> String {
        use rvision::help::Block;
        let mut s = String::new();
        for block in &t.body {
            match block {
                Block::Paragraph(p) => {
                    s.push_str(p);
                    s.push('\n');
                }
                Block::Preformatted(lines) => {
                    for l in lines {
                        s.push_str(l);
                        s.push('\n');
                    }
                }
            }
        }
        s
    }

    /// Extracts every `{label|target}` target from raw markup (links are reduced
    /// to label text at parse time, so this scans the source).
    fn link_targets(src: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = src;
        while let Some(o) = rest.find('{') {
            let after = &rest[o + 1..];
            if let Some(bar) = after.find('|') {
                let ab = &after[bar + 1..];
                if let Some(close) = ab.find('}') {
                    out.push(ab[..close].to_string());
                    rest = &ab[close + 1..];
                    continue;
                }
            }
            rest = after;
        }
        out
    }

    #[test]
    fn shipped_content_parses_with_the_expected_topics() {
        let c = HelpContents::parse(HELP_TEXT);
        let ids: Vec<&str> = c.topics().iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "overview",
                "keyboard",
                "clipboard",
                "files",
                "find",
                "settings"
            ]
        );
    }

    #[test]
    fn shipped_topic_ids_are_unique() {
        let c = HelpContents::parse(HELP_TEXT);
        let mut seen = std::collections::BTreeSet::new();
        for t in c.topics() {
            assert!(seen.insert(t.id.clone()), "duplicate topic id {:?}", t.id);
        }
    }

    #[test]
    fn the_clipboard_topic_documents_both_pastes() {
        let c = HelpContents::parse(HELP_TEXT);
        let text = topic_text(c.topic("clipboard").expect("a clipboard topic"));
        assert!(text.contains("Ctrl+V"), "internal paste key documented");
        assert!(text.contains("Ctrl+Shift+V"), "system paste key documented");
    }

    #[test]
    fn every_link_target_resolves() {
        let c = HelpContents::parse(HELP_TEXT);
        for target in link_targets(HELP_TEXT) {
            assert!(
                c.topic(&target).is_some(),
                "dangling help link target {target:?}"
            );
        }
    }

    #[test]
    fn clicking_a_topic_focuses_the_list_and_shows_it() {
        let mut v = viewer(None);
        // Move focus to the page first, then click a list row to come back.
        press(&mut v, KeyCode::Tab);
        assert_eq!(v.focus, FOCUS_PAGE);
        // Row 2 of the list = "Clipboard"; interior origin is (1,1), list at (0,0).
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        let click = Event::Mouse(MouseEvent {
            kind: MouseKind::Down(MouseButton::Left),
            pos: Point::new(3, 1 + 2),
            modifiers: Modifiers::NONE,
        });
        v.handle_event(&click, &mut ctx);
        assert_eq!(v.focus, FOCUS_LIST);
        assert_eq!(v.list.selected(), Some(2));
        assert!(page_text(&v).contains("editor clipboard"));
    }
}
