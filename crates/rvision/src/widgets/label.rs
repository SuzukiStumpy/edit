//! A static text label for a dialog (TurboVision's `TLabel`, minus the hot-key
//! link).
//!
//! A non-focusable line of text in the dialog's [`Role::DialogBackground`]
//! colour. TurboVision labels can carry a hot-key that focuses an associated
//! control; that needs view IDs and is deferred (see `controls.md`). For now a
//! label just draws its text.

use crate::canvas::Canvas;
use crate::color::Style;
use crate::geometry::{Point, Rect};
use crate::theme::{Role, Theme};
use crate::view::View;

/// A line of descriptive text in a dialog.
pub struct Label {
    bounds: Rect,
    text: String,
    style: Style,
}

impl Label {
    /// Creates a label at `bounds` showing `text`, in the theme's dialog colour.
    pub fn new(bounds: Rect, text: &str, theme: &Theme) -> Self {
        Self {
            bounds,
            text: text.to_string(),
            style: theme.style(Role::DialogBackground),
        }
    }

    /// Overrides the label's style (e.g. for an emphasised line).
    pub fn with_style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

impl View for Label {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.put_str(Point::new(0, 0), &self.text, self.style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Buffer;
    use crate::geometry::Size;

    fn rect(x: i16, y: i16, w: i16, h: i16) -> Rect {
        Rect::from_origin_size(Point::new(x, y), Size::new(w, h))
    }

    #[test]
    fn draws_its_text_and_is_not_focusable() {
        let label = Label::new(rect(0, 0, 12, 1), "Name:", &Theme::default());
        assert!(!label.focusable());

        let mut buf = Buffer::new(Size::new(12, 1));
        let mut c = Canvas::new(&mut buf);
        label.draw(&mut c);
        assert_eq!(buf.to_text(), "Name:       ");
    }

    #[test]
    fn uses_the_dialog_colour() {
        let theme = Theme::default();
        let label = Label::new(rect(0, 0, 6, 1), "Hi", &theme);
        let mut buf = Buffer::new(Size::new(6, 1));
        let mut c = Canvas::new(&mut buf);
        label.draw(&mut c);
        assert_eq!(
            buf.get(Point::new(0, 0)).unwrap().style(),
            theme.style(Role::DialogBackground)
        );
    }
}
