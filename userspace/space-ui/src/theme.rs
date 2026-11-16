//! XFCE-ergonomic + KDE Breeze-inspired color palette and spacing metrics.

use space_core::Color;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub bg_window: Color,
    pub bg_panel: Color,
    pub bg_surface: Color,
    pub bg_surface_alt: Color,
    pub bg_hover: Color,
    pub bg_active: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub danger: Color,
    pub success: Color,
    pub warning: Color,
    pub border: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_on_accent: Color,
    pub radius: i32,
}

impl Theme {
    pub const BREEZE_DARK: Self = Self {
        bg_window: Color::rgb(26, 29, 36),
        bg_panel: Color::argb(238, 20, 23, 30),
        bg_surface: Color::rgb(34, 38, 48),
        bg_surface_alt: Color::rgb(41, 46, 58),
        bg_hover: Color::rgb(52, 59, 74),
        bg_active: Color::rgb(62, 72, 92),
        accent: Color::rgb(61, 174, 233),       // KDE Breeze cyan-blue
        accent_soft: Color::argb(90, 61, 174, 233),
        danger: Color::rgb(218, 68, 83),
        success: Color::rgb(39, 174, 96),
        warning: Color::rgb(246, 116, 0),
        border: Color::rgb(64, 72, 88),
        text_primary: Color::rgb(239, 240, 241),
        text_secondary: Color::rgb(161, 169, 177),
        text_on_accent: Color::rgb(255, 255, 255),
        radius: 6,
    };
}
