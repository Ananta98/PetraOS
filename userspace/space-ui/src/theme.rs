//! Color palettes and 3D bevel/spacing metrics (XP Luna Blue & KDE Breeze Dark).

use space_core::Color;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub bg_window: Color,
    pub bg_panel: Color,
    pub bg_surface: Color,
    pub bg_surface_alt: Color,
    pub bg_input: Color,
    pub bg_hover: Color,
    pub bg_active: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub danger: Color,
    pub success: Color,
    pub warning: Color,
    pub border: Color,
    pub bevel_light: Color,
    pub bevel_shadow: Color,
    pub bevel_dark: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_on_accent: Color,
    pub radius: i32,
    pub is_xp: bool,
}

impl Theme {
    pub const BREEZE_DARK: Self = Self {
        bg_window: Color::rgb(26, 29, 36),
        bg_panel: Color::argb(238, 20, 23, 30),
        bg_surface: Color::rgb(34, 38, 48),
        bg_surface_alt: Color::rgb(41, 46, 58),
        bg_input: Color::rgb(20, 23, 29),
        bg_hover: Color::rgb(52, 59, 74),
        bg_active: Color::rgb(62, 72, 92),
        accent: Color::rgb(61, 174, 233),
        accent_soft: Color::argb(90, 61, 174, 233),
        danger: Color::rgb(218, 68, 83),
        success: Color::rgb(39, 174, 96),
        warning: Color::rgb(246, 116, 0),
        border: Color::rgb(64, 72, 88),
        bevel_light: Color::rgb(82, 90, 108),
        bevel_shadow: Color::rgb(20, 23, 30),
        bevel_dark: Color::rgb(12, 14, 18),
        text_primary: Color::rgb(239, 240, 241),
        text_secondary: Color::rgb(161, 169, 177),
        text_on_accent: Color::rgb(255, 255, 255),
        radius: 6,
        is_xp: false,
    };

    /// Classic Windows XP (Luna Blue) theme.
    pub const XP_LUNA: Self = Self {
        bg_window: Color::rgb(0xEC, 0xE9, 0xD8),      // #ECE9D8 warm beige-gray
        bg_panel: Color::rgb(0x24, 0x5E, 0xDB),       // #245EDB taskbar blue
        bg_surface: Color::rgb(0xFF, 0xFF, 0xFF),     // #FFFFFF client/list area
        bg_surface_alt: Color::rgb(0xF4, 0xF3, 0xEE), // subtle raised control face
        bg_input: Color::rgb(0xFF, 0xFF, 0xFF),       // #FFFFFF input well
        bg_hover: Color::rgb(0xE8, 0xEE, 0xFA),
        bg_active: Color::rgb(0x31, 0x6A, 0xC5),      // #316AC5 XP selection blue
        accent: Color::rgb(0x00, 0x54, 0xE3),         // #0054E3 Luna active blue
        accent_soft: Color::rgb(0xD3, 0xE5, 0xFA),    // #D3E5FA Start Menu right column blue
        danger: Color::rgb(0xE5, 0x51, 0x32),         // #E55132 glossy XP close red
        success: Color::rgb(0x3C, 0x9E, 0x3C),        // #3C9E3C XP Start green
        warning: Color::rgb(0xF8, 0xB3, 0x30),
        border: Color::rgb(0x7F, 0x9D, 0xB9),         // #7F9DB9 XP Luna control border
        bevel_light: Color::rgb(0xFF, 0xFF, 0xFF),    // #FFFFFF 3D highlight
        bevel_shadow: Color::rgb(0xAC, 0xA8, 0x99),   // #ACA899 3D shadow
        bevel_dark: Color::rgb(0x71, 0x6F, 0x64),     // #716F64 3D dark shadow
        text_primary: Color::rgb(0x00, 0x00, 0x00),
        text_secondary: Color::rgb(0x55, 0x53, 0x4C),
        text_on_accent: Color::rgb(0xFF, 0xFF, 0xFF),
        radius: 3,
        is_xp: true,
    };

    // XP Luna chrome constants
    pub const XP_TITLE_ACTIVE_TOP: Color = Color::rgb(0x00, 0x54, 0xE3);
    pub const XP_TITLE_ACTIVE_MID: Color = Color::rgb(0x28, 0x8E, 0xF7);
    pub const XP_TITLE_ACTIVE_BOT: Color = Color::rgb(0x00, 0x54, 0xE3);
    pub const XP_TITLE_ACTIVE_BORDER: Color = Color::rgb(0x00, 0x3D, 0xDC);
    pub const XP_TITLE_INACTIVE_TOP: Color = Color::rgb(0x7A, 0x96, 0xDF);
    pub const XP_TITLE_INACTIVE_BOT: Color = Color::rgb(0x9D, 0xB9, 0xEB);
    pub const XP_BTN_MINMAX: Color = Color::rgb(0x3C, 0x81, 0xF3);
    pub const XP_TASKBAR_TOP_HI: Color = Color::rgb(0x3D, 0x95, 0xFF);
    pub const XP_TASKBAR_BLUE: Color = Color::rgb(0x24, 0x5E, 0xDB);
    pub const XP_TASKBAR_BOT: Color = Color::rgb(0x19, 0x41, 0xA5);
    pub const XP_START_TOP: Color = Color::rgb(0x3C, 0x9E, 0x3C);
    pub const XP_START_BOT: Color = Color::rgb(0x2E, 0x8B, 0x2E);
    pub const XP_TRAY_BLUE: Color = Color::rgb(0x0F, 0x8D, 0xF5);
}
