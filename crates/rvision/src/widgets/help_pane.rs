//! The help page pane: a read-only, vertically-scrolling renderer for one
//! [`HelpTopic`](crate::help::HelpTopic) (ADR 0023, `docs/specs/help.md`).
//!
//! It reflows [`Paragraph`](crate::help::Block::Paragraph) prose to its current
//! width (via [`wrap`](crate::wrap)) and emits
//! [`Preformatted`](crate::help::Block::Preformatted) lines verbatim, so prose
//! adapts to the pane while keybinding tables stay aligned. A vertical
//! [`ScrollBar`](super::ScrollBar) appears down the right edge only when the page
//! overflows; the wheel and the bar's arrows/track scroll it. It is the reusable
//! part shared by the framework's (future) help window and the editor's modal
//! help viewer — neither owns the rendering.

use crate::canvas::Canvas;
use crate::cell::Cell;
use crate::color::Style;
use crate::event::{Event, EventResult, KeyCode, MouseButton, MouseEvent, MouseKind};
use crate::geometry::{Point, Rect, Size};
use crate::help::{Block, HelpTopic};
use crate::theme::{Role, Theme};
use crate::view::{Context, View};
use crate::wrap;

use super::{ScrollBar, ScrollPart};
use unicode_width::UnicodeWidthStr;

/// Lines panned per wheel notch — matches the editor's feel.
const WHEEL_STEP: isize = 3;

/// A scrollable, read-only view of one help topic's body.
pub struct HelpPane {
    bounds: Rect,
    /// The current topic's blocks, kept so the pane can re-lay-out on resize.
    body: Vec<Block>,
    /// The laid-out display lines at the current width.
    lines: Vec<String>,
    /// Index of the topmost visible line.
    top: usize,
    focused: bool,
    style: Style,
}

impl HelpPane {
    /// Creates an empty pane at `bounds`.
    pub fn new(bounds: Rect, theme: &Theme) -> Self {
        Self {
            bounds,
            body: Vec::new(),
            lines: Vec::new(),
            top: 0,
            focused: false,
            style: theme.style(Role::DialogBackground),
        }
    }

    /// Shows `topic`: lays its body out for the current width and scrolls to the
    /// top.
    pub fn show(&mut self, topic: &HelpTopic) {
        self.body = topic.body.clone();
        self.top = 0;
        self.layout();
    }

    /// Repositions/resizes the pane, re-laying-out if the width changed.
    pub fn set_bounds(&mut self, bounds: Rect) {
        let rewidth = bounds.width() != self.bounds.width();
        self.bounds = bounds;
        if rewidth {
            self.layout();
        }
        self.clamp_top();
    }

    /// The total number of laid-out lines (for sizing / overflow checks).
    pub fn content_height(&self) -> i16 {
        self.lines.len() as i16
    }

    /// The widest laid-out line in display columns (for sizing).
    pub fn content_width(&self) -> i16 {
        self.lines
            .iter()
            .map(|l| l.width() as i16)
            .max()
            .unwrap_or(0)
    }

    /// Number of visible text rows.
    fn rows(&self) -> usize {
        self.bounds.height().max(0) as usize
    }

    /// Whether a vertical scroll bar is currently needed.
    fn needs_bar(&self) -> bool {
        self.lines.len() > self.rows() && self.bounds.width() > 1
    }

    /// Lays the body out to the current text width (one column narrower when a
    /// scroll bar is needed). A first pass at full width decides the bar; since a
    /// narrower width never *reduces* the line count, the decision is stable.
    fn layout(&mut self) {
        let full = self.bounds.width().max(0) as u16;
        let at_full = render_blocks(&self.body, full);
        let needs_bar = at_full.len() > self.rows() && full > 1;
        self.lines = if needs_bar {
            render_blocks(&self.body, full - 1)
        } else {
            at_full
        };
        self.clamp_top();
    }

    /// The largest valid `top`, keeping the last screenful in view.
    fn max_top(&self) -> usize {
        self.lines.len().saturating_sub(self.rows())
    }

    fn clamp_top(&mut self) {
        self.top = self.top.min(self.max_top());
    }

    /// Scrolls by `delta` lines (negative = up), clamped.
    fn scroll_by(&mut self, delta: isize) {
        let max = self.max_top() as isize;
        self.top = ((self.top as isize) + delta).clamp(0, max) as usize;
    }

    /// Handles a mouse event in the pane's local coordinates: the wheel pans, and
    /// (when shown) the scroll bar's arrows/track scroll. Works regardless of
    /// focus, so the wheel acts under the pointer.
    fn handle_mouse(&mut self, m: &MouseEvent) -> EventResult {
        let rows = self.rows();
        let page = rows.max(1) as isize;
        match m.kind {
            MouseKind::ScrollDown => {
                self.scroll_by(WHEEL_STEP);
                EventResult::Consumed
            }
            MouseKind::ScrollUp => {
                self.scroll_by(-WHEEL_STEP);
                EventResult::Consumed
            }
            MouseKind::Down(MouseButton::Left) if self.needs_bar() => {
                let width = self.bounds.width();
                if m.pos.x != width - 1 {
                    return EventResult::Ignored;
                }
                let mut bar = ScrollBar::new(
                    Rect::from_origin_size(Point::new(width - 1, 0), Size::new(1, rows as i16)),
                    self.style,
                );
                bar.set_metrics(self.lines.len(), rows, self.top);
                match bar.hit(m.pos) {
                    Some(ScrollPart::LineUp) => self.scroll_by(-1),
                    Some(ScrollPart::LineDown) => self.scroll_by(1),
                    Some(ScrollPart::PageUp) => self.scroll_by(-page),
                    Some(ScrollPart::PageDown) => self.scroll_by(page),
                    _ => {}
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

/// Renders `body` to display lines at `width` columns: each block is reflowed
/// (paragraphs) or kept verbatim (preformatted), with one blank line between
/// blocks.
fn render_blocks(body: &[Block], width: u16) -> Vec<String> {
    let mut out = Vec::new();
    for (i, block) in body.iter().enumerate() {
        if i > 0 {
            out.push(String::new());
        }
        match block {
            Block::Paragraph(text) => out.extend(wrap::wrap(text, width)),
            Block::Preformatted(lines) => out.extend(lines.iter().cloned()),
        }
    }
    out
}

impl View for HelpPane {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn draw(&self, canvas: &mut Canvas) {
        let area = canvas.bounds();
        canvas.fill(area, &Cell::blank(self.style));
        let rows = self.rows();
        if rows == 0 || area.width() <= 0 {
            return;
        }
        let needs_bar = self.needs_bar();
        let text_w = if needs_bar {
            area.width() - 1
        } else {
            area.width()
        };

        {
            let mut text = canvas.child(Rect::from_origin_size(
                Point::new(0, 0),
                Size::new(text_w, rows as i16),
            ));
            for r in 0..rows {
                let idx = self.top + r;
                if idx >= self.lines.len() {
                    break;
                }
                text.put_str(Point::new(0, r as i16), &self.lines[idx], self.style);
            }
        }

        if needs_bar {
            let mut bar = ScrollBar::new(
                Rect::from_origin_size(Point::new(0, 0), Size::new(1, rows as i16)),
                self.style,
            );
            bar.set_metrics(self.lines.len(), rows, self.top);
            let mut sub = canvas.child(Rect::from_origin_size(
                Point::new(text_w, 0),
                Size::new(1, rows as i16),
            ));
            bar.draw(&mut sub);
        }
    }

    fn handle_event(&mut self, event: &Event, _ctx: &mut Context) -> EventResult {
        if let Event::Mouse(m) = event {
            return self.handle_mouse(m);
        }
        if let Event::Key(key) = event {
            if !self.focused {
                return EventResult::Ignored;
            }
            let page = self.rows().max(1) as isize;
            match key.code {
                KeyCode::Up => self.scroll_by(-1),
                KeyCode::Down => self.scroll_by(1),
                KeyCode::PageUp => self.scroll_by(-page),
                KeyCode::PageDown => self.scroll_by(page),
                KeyCode::Home => self.top = 0,
                KeyCode::End => self.top = self.max_top(),
                _ => return EventResult::Ignored,
            }
            return EventResult::Consumed;
        }
        EventResult::Ignored
    }

    fn focusable(&self) -> bool {
        true
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::command::CommandSet;
    use crate::event::{KeyEvent, Modifiers};
    use crate::help::Block;

    fn rect(w: i16, h: i16) -> Rect {
        Rect::from_origin_size(Point::new(0, 0), Size::new(w, h))
    }

    fn topic(body: Vec<Block>) -> HelpTopic {
        HelpTopic {
            id: "t".into(),
            title: "T".into(),
            body,
        }
    }

    fn pane(w: i16, h: i16, body: Vec<Block>) -> HelpPane {
        let mut p = HelpPane::new(rect(w, h), &Theme::default());
        p.show(&topic(body));
        p.set_focused(true);
        p
    }

    fn press(p: &mut HelpPane, code: KeyCode) -> EventResult {
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        p.handle_event(&Event::Key(KeyEvent::new(code, Modifiers::NONE)), &mut ctx)
    }

    fn wheel(p: &mut HelpPane, kind: MouseKind) {
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        p.handle_event(
            &Event::Mouse(MouseEvent {
                kind,
                pos: Point::new(1, 1),
                modifiers: Modifiers::NONE,
            }),
            &mut ctx,
        );
    }

    fn render(p: &HelpPane, w: i16, h: i16) -> String {
        let mut buf = Buffer::new(Size::new(w, h));
        let mut canvas = Canvas::new(&mut buf);
        p.draw(&mut canvas);
        buf.to_text()
    }

    #[test]
    fn prose_reflows_but_preformatted_stays_verbatim() {
        // Pane 14 wide: prose wraps at the space boundary, the 13-wide table fits.
        let p = pane(
            14,
            10,
            vec![
                Block::Paragraph("the quick brown fox jumps".into()),
                Block::Preformatted(vec!["Ctrl+S   Save".into(), "F3       Next".into()]),
            ],
        );
        let text = render(&p, 14, 10);
        let rows: Vec<&str> = text.lines().collect();
        // Prose wrapped to <= 12 columns.
        assert_eq!(rows[0].trim_end(), "the quick");
        assert_eq!(rows[1].trim_end(), "brown fox");
        assert_eq!(rows[2].trim_end(), "jumps");
        // A blank line between blocks, then the table verbatim (columns intact).
        assert_eq!(rows[3].trim_end(), "");
        assert_eq!(rows[4].trim_end(), "Ctrl+S   Save");
        assert_eq!(rows[5].trim_end(), "F3       Next");
    }

    #[test]
    fn a_short_page_shows_no_scroll_bar() {
        let p = pane(20, 6, vec![Block::Paragraph("one short line".into())]);
        assert!(!p.needs_bar());
        let text = render(&p, 20, 6);
        // No bar glyphs in the last column.
        for row in text.lines() {
            assert!(!row.ends_with('▲') && !row.ends_with('▼'));
        }
    }

    #[test]
    fn an_overflowing_page_shows_a_scroll_bar() {
        let body = vec![Block::Preformatted(
            (0..20).map(|i| format!("line {i}")).collect(),
        )];
        let p = pane(12, 5, body);
        assert!(p.needs_bar());
        assert_eq!(p.content_height(), 20);
        let rows: Vec<String> = render(&p, 12, 5).lines().map(str::to_string).collect();
        assert!(rows[0].ends_with('▲'), "up arrow at the top of the bar");
        assert!(rows[4].ends_with('▼'), "down arrow at the foot");
    }

    #[test]
    fn keys_scroll_and_clamp() {
        let body = vec![Block::Preformatted(
            (0..20).map(|i| format!("L{i}")).collect(),
        )];
        let mut p = pane(10, 5, body); // 20 lines, 5 rows → max_top 15
        assert_eq!(p.top, 0);
        press(&mut p, KeyCode::Down);
        assert_eq!(p.top, 1);
        press(&mut p, KeyCode::PageDown); // + 5 rows
        assert_eq!(p.top, 6);
        press(&mut p, KeyCode::End);
        assert_eq!(p.top, 15);
        press(&mut p, KeyCode::Down); // clamps at the bottom
        assert_eq!(p.top, 15);
        press(&mut p, KeyCode::Home);
        assert_eq!(p.top, 0);
        press(&mut p, KeyCode::Up); // clamps at the top
        assert_eq!(p.top, 0);
    }

    #[test]
    fn the_wheel_pans_the_page() {
        let body = vec![Block::Preformatted(
            (0..20).map(|i| format!("L{i}")).collect(),
        )];
        let mut p = pane(10, 5, body);
        wheel(&mut p, MouseKind::ScrollDown);
        assert_eq!(p.top, WHEEL_STEP as usize);
        wheel(&mut p, MouseKind::ScrollUp);
        assert_eq!(p.top, 0);
    }

    #[test]
    fn keys_are_ignored_when_unfocused_so_they_bubble() {
        let body = vec![Block::Preformatted(
            (0..20).map(|i| format!("L{i}")).collect(),
        )];
        let mut p = pane(10, 5, body);
        p.set_focused(false);
        assert_eq!(press(&mut p, KeyCode::Down), EventResult::Ignored);
        assert_eq!(p.top, 0);
    }

    #[test]
    fn show_resets_scroll_to_the_top() {
        let long = vec![Block::Preformatted(
            (0..20).map(|i| format!("L{i}")).collect(),
        )];
        let mut p = pane(10, 5, long);
        press(&mut p, KeyCode::End);
        assert!(p.top > 0);
        p.show(&topic(vec![Block::Paragraph("fresh".into())]));
        assert_eq!(p.top, 0);
    }
}
