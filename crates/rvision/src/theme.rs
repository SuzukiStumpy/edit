//! Semantic styling roles and the theme that resolves them (ADR 0005).
//!
//! Widgets ask for a [`Role`] — what a piece of UI *is* — rather than naming
//! colours. A [`Theme`] maps every role to a concrete [`Style`], so swapping the
//! theme re-skins the entire interface. One default 16-colour CGA theme ships
//! here; alternative (dark, monochrome) themes can be added later without
//! touching widget code.

use crate::color::{Color, Color16, Style};

/// A semantic role describing what a piece of UI is, independent of its colours.
///
/// `Role` is used to index a [`Theme`]'s style table via `role as usize`, so the
/// variants and [`Role::ALL`] must stay in the same (discriminant) order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// The desktop backdrop behind all windows.
    DesktopBackground,
    /// A window's border/frame.
    WindowFrame,
    /// A window's title text.
    WindowTitle,
    /// The top menu bar background.
    MenuBar,
    /// The currently highlighted menu item.
    MenuSelected,
    /// A disabled (greyed) menu item.
    MenuDisabled,
    /// The bottom status bar background.
    StatusBar,
    /// A highlighted shortcut key in the status bar.
    StatusKey,
    /// A button at rest.
    ButtonNormal,
    /// A focused button.
    ButtonFocused,
    /// Normal text in an editor window.
    EditorText,
    /// Selected (highlighted) text.
    Selection,
}

impl Role {
    /// Every role, in discriminant order (so `ALL[i] as usize == i`).
    pub const ALL: [Role; 12] = [
        Role::DesktopBackground,
        Role::WindowFrame,
        Role::WindowTitle,
        Role::MenuBar,
        Role::MenuSelected,
        Role::MenuDisabled,
        Role::StatusBar,
        Role::StatusKey,
        Role::ButtonNormal,
        Role::ButtonFocused,
        Role::EditorText,
        Role::Selection,
    ];

    /// The number of roles.
    pub const COUNT: usize = Self::ALL.len();
}

/// Resolves [`Role`]s to concrete [`Style`]s. Clone and [`Theme::with`] to
/// derive a variant; swap the whole thing to re-skin the UI.
#[derive(Debug, Clone)]
pub struct Theme {
    styles: [Style; Role::COUNT],
}

impl Theme {
    /// Returns the style associated with `role`.
    pub fn style(&self, role: Role) -> Style {
        self.styles[role as usize]
    }

    /// Returns the theme with `role`'s style replaced, leaving all other roles
    /// untouched.
    pub fn with(mut self, role: Role, style: Style) -> Self {
        self.styles[role as usize] = style;
        self
    }
}

impl Default for Theme {
    /// The default 16-colour CGA theme. Colour choices are provisional and may be
    /// tuned as widgets are built.
    fn default() -> Self {
        let cga = |fg: Color16, bg: Color16| Style::new().fg(Color::Named(fg)).bg(Color::Named(bg));
        let mut styles = [Style::new(); Role::COUNT];
        styles[Role::DesktopBackground as usize] = cga(Color16::LightGray, Color16::Blue);
        styles[Role::WindowFrame as usize] = cga(Color16::White, Color16::Blue);
        styles[Role::WindowTitle as usize] = cga(Color16::White, Color16::Blue);
        styles[Role::MenuBar as usize] = cga(Color16::Black, Color16::LightGray);
        styles[Role::MenuSelected as usize] = cga(Color16::Black, Color16::Green);
        styles[Role::MenuDisabled as usize] = cga(Color16::DarkGray, Color16::LightGray);
        styles[Role::StatusBar as usize] = cga(Color16::Black, Color16::LightGray);
        styles[Role::StatusKey as usize] = cga(Color16::Red, Color16::LightGray);
        styles[Role::ButtonNormal as usize] = cga(Color16::Black, Color16::Green);
        styles[Role::ButtonFocused as usize] = cga(Color16::White, Color16::Green);
        styles[Role::EditorText as usize] = cga(Color16::White, Color16::Blue);
        styles[Role::Selection as usize] = cga(Color16::Black, Color16::Cyan);
        Self { styles }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cga(fg: Color16, bg: Color16) -> Style {
        Style::new().fg(Color::Named(fg)).bg(Color::Named(bg))
    }

    // Tracer bullet: the default theme resolves known roles to CGA styles.
    #[test]
    fn default_theme_resolves_known_roles() {
        let t = Theme::default();
        assert_eq!(
            t.style(Role::DesktopBackground),
            cga(Color16::LightGray, Color16::Blue)
        );
        assert_eq!(
            t.style(Role::MenuSelected),
            cga(Color16::Black, Color16::Green)
        );
        assert_eq!(
            t.style(Role::EditorText),
            cga(Color16::White, Color16::Blue)
        );
    }

    #[test]
    fn role_all_is_in_discriminant_order() {
        assert_eq!(Role::COUNT, Role::ALL.len());
        for (i, role) in Role::ALL.iter().enumerate() {
            assert_eq!(*role as usize, i, "Role::ALL[{i}] is out of order");
        }
    }

    #[test]
    fn with_overrides_one_role_only() {
        let custom = cga(Color16::Yellow, Color16::Black);
        let t = Theme::default().with(Role::EditorText, custom);
        // The overridden role takes the new style...
        assert_eq!(t.style(Role::EditorText), custom);
        // ...and an untouched role keeps its default.
        assert_eq!(
            t.style(Role::MenuSelected),
            cga(Color16::Black, Color16::Green)
        );
    }
}
