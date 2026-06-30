//! The help page pane: a read-only renderer for one
//! [`HelpTopic`](crate::help::HelpTopic) that scrolls both ways (ADR 0023,
//! `docs/specs/help.md`).
//!
//! It reflows [`Paragraph`](crate::help::Block::Paragraph) prose to its current
//! width (via [`wrap`](crate::wrap)) and emits
//! [`Preformatted`](crate::help::Block::Preformatted) lines verbatim, so prose
//! adapts to the pane while keybinding tables and other fixed-format blocks stay
//! aligned. Because a `<pre>` block can be wider than the pane, the pane scrolls
//! **horizontally** as well as vertically: a [`ScrollBar`](super::ScrollBar)
//! appears down the right edge when the page is too tall and along the bottom when
//! a line is too wide, each only when needed (the two interact — one steals a
//! row/column, which can call for the other, so they are decided together). Arrow
//! keys, the wheel, and the bars' arrows/track all scroll. It is the reusable part
//! shared by the framework's (future) help window and the editor's modal help
//! viewer — neither owns the rendering.

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

/// A read-only view of one help topic's body that scrolls in both axes.
pub struct HelpPane {
    bounds: Rect,
    /// The current topic's blocks, kept so the pane can re-lay-out on resize.
    body: Vec<Block>,
    /// The laid-out display lines at the current text width.
    lines: Vec<String>,
    /// Index of the topmost visible line.
    top: usize,
    /// Leftmost visible display column (horizontal scroll offset).
    left: usize,
    /// Whether each scroll bar is currently shown (decided together in `layout`).
    needs_vbar: bool,
    needs_hbar: bool,
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
            left: 0,
            needs_vbar: false,
            needs_hbar: false,
            focused: false,
            style: theme.style(Role::DialogBackground),
        }
    }

    /// Shows `topic`: lays its body out for the current size and scrolls to the
    /// top-left.
    pub fn show(&mut self, topic: &HelpTopic) {
        self.body = topic.body.clone();
        self.top = 0;
        self.left = 0;
        self.layout();
    }

    /// Repositions/resizes the pane, re-laying-out if the size changed.
    pub fn set_bounds(&mut self, bounds: Rect) {
        let resized = bounds.size() != self.bounds.size();
        self.bounds = bounds;
        if resized {
            self.layout();
        } else {
            self.clamp_top();
            self.clamp_left();
        }
    }

    /// The total number of laid-out lines (for sizing / overflow checks).
    pub fn content_height(&self) -> i16 {
        self.lines.len() as i16
    }

    /// The widest laid-out line in display columns (for sizing).
    pub fn content_width(&self) -> i16 {
        max_line_width(&self.lines)
    }

    /// The pane's full height in rows (before any horizontal bar is subtracted).
    fn rows_total(&self) -> usize {
        self.bounds.height().max(0) as usize
    }

    /// Visible text rows — the height minus the horizontal bar's row, if shown.
    fn text_rows(&self) -> usize {
        self.rows_total()
            .saturating_sub(usize::from(self.needs_hbar))
    }

    /// Visible text columns — the width minus the vertical bar's column, if shown.
    fn text_w(&self) -> i16 {
        (self.bounds.width() - i16::from(self.needs_vbar)).max(0)
    }

    /// Decides which scroll bars are needed and lays the body out to match. The
    /// two bars interact — a vertical bar narrows the text (which can make a line
    /// overflow → horizontal bar) and a horizontal bar shortens it (which can push
    /// the line count over → vertical bar) — so they are found together by a short
    /// fixed-point iteration (it converges in a step or two; the cap just bounds
    /// it). Only the *width* changes the wrapped line content, so the body is
    /// re-rendered once at the settled text width.
    fn layout(&mut self) {
        let width = self.bounds.width().max(0);
        let total_rows = self.rows_total();
        let (mut vbar, mut hbar) = (false, false);
        for _ in 0..4 {
            let text_w = (width - i16::from(vbar)).max(0);
            let text_rows = total_rows.saturating_sub(usize::from(hbar));
            let lines = render_blocks(&self.body, text_w as u16);
            let new_vbar = lines.len() > text_rows && width > 1;
            let new_hbar = max_line_width(&lines) > text_w && total_rows > 1;
            if (new_vbar, new_hbar) == (vbar, hbar) {
                break;
            }
            vbar = new_vbar;
            hbar = new_hbar;
        }
        self.needs_vbar = vbar;
        self.needs_hbar = hbar;
        let text_w = (width - i16::from(vbar)).max(0);
        self.lines = render_blocks(&self.body, text_w as u16);
        self.clamp_top();
        self.clamp_left();
    }

    /// The largest valid `top`, keeping the last screenful in view.
    fn max_top(&self) -> usize {
        self.lines.len().saturating_sub(self.text_rows())
    }

    /// The largest valid `left`, keeping the rightmost column reachable.
    fn max_left(&self) -> usize {
        (self.content_width().max(0) as usize).saturating_sub(self.text_w().max(0) as usize)
    }

    fn clamp_top(&mut self) {
        self.top = self.top.min(self.max_top());
    }

    fn clamp_left(&mut self) {
        self.left = self.left.min(self.max_left());
    }

    /// Scrolls by `delta` lines (negative = up), clamped.
    fn scroll_by(&mut self, delta: isize) {
        let max = self.max_top() as isize;
        self.top = ((self.top as isize) + delta).clamp(0, max) as usize;
    }

    /// Scrolls by `delta` columns (negative = left), clamped.
    fn scroll_h_by(&mut self, delta: isize) {
        let max = self.max_left() as isize;
        self.left = ((self.left as isize) + delta).clamp(0, max) as usize;
    }

    /// Handles a mouse event in the pane's local coordinates: the wheel pans
    /// vertically, and (when shown) each scroll bar's arrows/track scroll its axis.
    /// Works regardless of focus, so the wheel acts under the pointer.
    fn handle_mouse(&mut self, m: &MouseEvent) -> EventResult {
        let text_w = self.text_w();
        let text_rows = self.text_rows();
        let vpage = text_rows.max(1) as isize;
        let hpage = text_w.max(1) as isize;
        match m.kind {
            MouseKind::ScrollDown => {
                self.scroll_by(WHEEL_STEP);
                EventResult::Consumed
            }
            MouseKind::ScrollUp => {
                self.scroll_by(-WHEEL_STEP);
                EventResult::Consumed
            }
            // The horizontal bar's row (checked first so the shared corner goes to
            // the vertical bar, matching the draw order).
            MouseKind::Down(MouseButton::Left)
                if self.needs_hbar && m.pos.y == text_rows as i16 && m.pos.x < text_w =>
            {
                let mut bar = ScrollBar::horizontal(
                    Rect::from_origin_size(Point::new(0, text_rows as i16), Size::new(text_w, 1)),
                    self.style,
                );
                bar.set_metrics(
                    self.content_width().max(0) as usize,
                    text_w as usize,
                    self.left,
                );
                match bar.hit(m.pos) {
                    Some(ScrollPart::LineUp) => self.scroll_h_by(-1),
                    Some(ScrollPart::LineDown) => self.scroll_h_by(1),
                    Some(ScrollPart::PageUp) => self.scroll_h_by(-hpage),
                    Some(ScrollPart::PageDown) => self.scroll_h_by(hpage),
                    _ => {}
                }
                EventResult::Consumed
            }
            MouseKind::Down(MouseButton::Left) if self.needs_vbar && m.pos.x == text_w => {
                let mut bar = ScrollBar::new(
                    Rect::from_origin_size(Point::new(text_w, 0), Size::new(1, text_rows as i16)),
                    self.style,
                );
                bar.set_metrics(self.lines.len(), text_rows, self.top);
                match bar.hit(m.pos) {
                    Some(ScrollPart::LineUp) => self.scroll_by(-1),
                    Some(ScrollPart::LineDown) => self.scroll_by(1),
                    Some(ScrollPart::PageUp) => self.scroll_by(-vpage),
                    Some(ScrollPart::PageDown) => self.scroll_by(vpage),
                    _ => {}
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }
}

/// The widest line in `lines`, in display columns.
fn max_line_width(lines: &[String]) -> i16 {
    lines.iter().map(|l| l.width() as i16).max().unwrap_or(0)
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
        let text_rows = self.text_rows();
        let text_w = self.text_w();
        if text_rows == 0 || text_w <= 0 {
            return;
        }

        {
            let mut text = canvas.child(Rect::from_origin_size(
                Point::new(0, 0),
                Size::new(text_w, text_rows as i16),
            ));
            // Draw each line shifted left by the scroll offset; the child canvas
            // clips the off-screen columns on both sides (`Canvas::set`).
            for r in 0..text_rows {
                let idx = self.top + r;
                if idx >= self.lines.len() {
                    break;
                }
                text.put_str(
                    Point::new(-(self.left as i16), r as i16),
                    &self.lines[idx],
                    self.style,
                );
            }
        }

        if self.needs_vbar {
            let mut bar = ScrollBar::new(
                Rect::from_origin_size(Point::new(0, 0), Size::new(1, text_rows as i16)),
                self.style,
            );
            bar.set_metrics(self.lines.len(), text_rows, self.top);
            let mut sub = canvas.child(Rect::from_origin_size(
                Point::new(text_w, 0),
                Size::new(1, text_rows as i16),
            ));
            bar.draw(&mut sub);
        }

        if self.needs_hbar {
            let mut bar = ScrollBar::horizontal(
                Rect::from_origin_size(Point::new(0, 0), Size::new(text_w, 1)),
                self.style,
            );
            bar.set_metrics(
                self.content_width().max(0) as usize,
                text_w as usize,
                self.left,
            );
            let mut sub = canvas.child(Rect::from_origin_size(
                Point::new(0, text_rows as i16),
                Size::new(text_w, 1),
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
            let vpage = self.text_rows().max(1) as isize;
            match key.code {
                KeyCode::Up => self.scroll_by(-1),
                KeyCode::Down => self.scroll_by(1),
                KeyCode::PageUp => self.scroll_by(-vpage),
                KeyCode::PageDown => self.scroll_by(vpage),
                KeyCode::Left => self.scroll_h_by(-1),
                KeyCode::Right => self.scroll_h_by(1),
                // Home returns to the top-left corner; End jumps to the last line.
                KeyCode::Home => {
                    self.top = 0;
                    self.left = 0;
                }
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

    fn click(p: &mut HelpPane, x: i16, y: i16) {
        let cs = CommandSet::new();
        let mut ctx = Context::new(&cs);
        p.handle_event(
            &Event::Mouse(MouseEvent {
                kind: MouseKind::Down(MouseButton::Left),
                pos: Point::new(x, y),
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
        assert!(!p.needs_vbar);
        assert!(!p.needs_hbar);
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
        assert!(p.needs_vbar);
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
    fn show_resets_scroll_to_the_top_left() {
        let long = vec![Block::Preformatted(vec![
            "a very wide preformatted line".into(),
        ])];
        let mut p = pane(10, 5, long);
        press(&mut p, KeyCode::Right);
        assert!(p.left > 0);
        p.show(&topic(vec![Block::Paragraph("fresh".into())]));
        assert_eq!(p.top, 0);
        assert_eq!(p.left, 0);
    }

    // --- horizontal scrolling (the wide-`<pre>` case) ---

    #[test]
    fn a_wide_preformatted_line_shows_a_horizontal_bar_and_scrolls() {
        // One 20-column line in an 8-wide pane: a horizontal bar, no vertical one.
        let body = vec![Block::Preformatted(vec!["0123456789ABCDEFGHIJ".into()])];
        let mut p = pane(8, 4, body);
        assert!(p.needs_hbar, "wide line needs a horizontal bar");
        assert!(!p.needs_vbar, "one line needs no vertical bar");
        assert_eq!(p.content_width(), 20);

        // The bar occupies the bottom row; the text rows are above it.
        let first = render(&p, 8, 4);
        let row0 = first.lines().next().unwrap();
        assert!(row0.starts_with("01234"), "left edge visible: {row0:?}");

        // Right scrolls the content; left of the line scrolls out of view.
        press(&mut p, KeyCode::Right);
        press(&mut p, KeyCode::Right);
        assert_eq!(p.left, 2);
        let scrolled = render(&p, 8, 4);
        let row0 = scrolled.lines().next().unwrap();
        assert!(row0.starts_with("23456"), "scrolled view: {row0:?}");

        // End-of-line clamp via repeated Right, then Home returns to the start.
        for _ in 0..50 {
            press(&mut p, KeyCode::Right);
        }
        assert_eq!(p.left, p.max_left());
        assert_eq!(p.left, 20 - p.text_w() as usize);
        press(&mut p, KeyCode::Home);
        assert_eq!(p.left, 0);
    }

    #[test]
    fn the_horizontal_bar_arrows_scroll_on_click() {
        let body = vec![Block::Preformatted(vec!["0123456789ABCDEFGHIJ".into()])];
        let mut p = pane(8, 4, body);
        // text_w 8, bar on the bottom row (y = rows_total - 1 = 3). Right arrow at
        // the bar's right end.
        let text_w = p.text_w();
        assert_eq!(p.text_rows(), 3); // one row given to the bar
        click(&mut p, text_w - 1, 3);
        assert_eq!(p.left, 1, "the right arrow advances one column");
    }

    #[test]
    fn both_bars_appear_for_a_tall_and_wide_page() {
        // Many wide lines: needs both bars; each steals a row/column.
        let body = vec![Block::Preformatted(
            (0..30)
                .map(|i| format!("{i:04}-wide-line-XXXXXXXXXXXXXXXX"))
                .collect(),
        )];
        let p = pane(10, 6, body);
        assert!(p.needs_vbar && p.needs_hbar);
        assert_eq!(p.text_w(), 9); // one column for the vertical bar
        assert_eq!(p.text_rows(), 5); // one row for the horizontal bar
    }
}
