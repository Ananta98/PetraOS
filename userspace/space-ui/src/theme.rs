//! Color palettes and modern-classic chrome metrics for PetraOS (Petra Slate & Petra Basalt).

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
    pub subtle_gradients: bool,
}

impl Theme {
    /// Default PetraOS desktop theme ("Petra Slate"): ReactOS-inspired crisp slate-cobalt
    /// chrome paired with clean cool-neutral surfaces and SerenityOS-style clarity.
    pub const PETRA_SLATE: Self = Self {
        bg_window: Color::rgb(0xEF, 0xF2, 0xF6),      // #EFF2F6 cool alabaster-slate
        bg_panel: Color::rgb(0x23, 0x4F, 0x8A),       // #234F8A deep slate-cobalt taskbar
        bg_surface: Color::rgb(0xFF, 0xFF, 0xFF),     // #FFFFFF crisp client/list area
        bg_surface_alt: Color::rgb(0xF6, 0xF8, 0xFB), // #F6F8FB subtle alternating row/control face
        bg_input: Color::rgb(0xFF, 0xFF, 0xFF),       // #FFFFFF input well
        bg_hover: Color::rgb(0xE4, 0xEF, 0xFB),       // #E4EFFB ice-blue hover tint
        bg_active: Color::rgb(0x2B, 0x6B, 0xB8),      // #2B6BB8 slate-cobalt selection
        accent: Color::rgb(0x23, 0x5F, 0xA8),         // #235FA8 primary cobalt accent
        accent_soft: Color::rgb(0xDC, 0xE9, 0xF8),    // #DCE9F8 soft slate-blue tint
        danger: Color::rgb(0xC9, 0x3B, 0x3B),         // #C93B3B crisp close/alert crimson
        success: Color::rgb(0x2E, 0x8B, 0x57),        // #2E8B57 clean emerald status
        warning: Color::rgb(0xD9, 0x82, 0x2B),        // #D9822B amber warning
        border: Color::rgb(0xB4, 0xBF, 0xC9),         // #B4BFC9 crisp 1px slate border
        bevel_light: Color::rgb(0xFF, 0xFF, 0xFF),    // #FFFFFF subtle top edge highlight
        bevel_shadow: Color::rgb(0xC6, 0xCE, 0xD8),   // #C6CED8 understated 1px separator
        bevel_dark: Color::rgb(0x88, 0x94, 0xA4),     // #8894A4 outer frame shadow
        text_primary: Color::rgb(0x18, 0x1E, 0x26),   // #181E26 deep charcoal ink
        text_secondary: Color::rgb(0x56, 0x61, 0x70), // #566170 muted slate label
        text_on_accent: Color::rgb(0xFF, 0xFF, 0xFF),
        radius: 2,
        subtle_gradients: true,
    };

    /// Dark companion theme ("Petra Basalt").
    pub const PETRA_BASALT: Self = Self {
        bg_window: Color::rgb(28, 32, 39),
        bg_panel: Color::rgb(21, 25, 31),
        bg_surface: Color::rgb(36, 41, 50),
        bg_surface_alt: Color::rgb(43, 49, 60),
        bg_input: Color::rgb(22, 25, 31),
        bg_hover: Color::rgb(52, 60, 74),
        bg_active: Color::rgb(48, 102, 168),
        accent: Color::rgb(68, 146, 224),
        accent_soft: Color::rgb(38, 62, 92),
        danger: Color::rgb(208, 72, 78),
        success: Color::rgb(46, 160, 98),
        warning: Color::rgb(224, 140, 48),
        border: Color::rgb(66, 75, 90),
        bevel_light: Color::rgb(82, 92, 110),
        bevel_shadow: Color::rgb(22, 25, 31),
        bevel_dark: Color::rgb(14, 16, 20),
        text_primary: Color::rgb(236, 240, 244),
        text_secondary: Color::rgb(156, 166, 178),
        text_on_accent: Color::rgb(255, 255, 255),
        radius: 2,
        subtle_gradients: true,
    };

    // Petra Slate window & panel chrome constants
    pub const TITLE_ACTIVE_HI: Color = Color::rgb(0x5B, 0x92, 0xD6);
    pub const TITLE_ACTIVE_TOP: Color = Color::rgb(0x35, 0x6D, 0xB5);
    pub const TITLE_ACTIVE_BOT: Color = Color::rgb(0x21, 0x4F, 0x8F);
    pub const TITLE_ACTIVE_BORDER: Color = Color::rgb(0x18, 0x3D, 0x72);
    pub const TITLE_INACTIVE_TOP: Color = Color::rgb(0xDC, 0xE2, 0xEA);
    pub const TITLE_INACTIVE_BOT: Color = Color::rgb(0xC7, 0xD0, 0xDC);
    pub const TITLE_INACTIVE_BORDER: Color = Color::rgb(0x8D, 0x99, 0xA8);
    pub const TITLE_BTN_BG: Color = Color::rgb(0x3E, 0x76, 0xBE);
    pub const PANEL_TOP_HI: Color = Color::rgb(0x4A, 0x82, 0xC8);
    pub const PANEL_TOP: Color = Color::rgb(0x2B, 0x5B, 0x9A);
    pub const PANEL_BOT: Color = Color::rgb(0x1E, 0x43, 0x76);
    pub const PANEL_BORDER: Color = Color::rgb(0x14, 0x31, 0x5A);
    pub const TRAY_TOP: Color = Color::rgb(0x23, 0x4E, 0x88);
    pub const TRAY_BOT: Color = Color::rgb(0x19, 0x3A, 0x68);
}
