// SPDX-License-Identifier: MPL-2.0

//! Framebuffer Console Font Manager with Unicode box-drawing, block elements, and symbol fallback.

use os_terminal::font::{BitmapFont, ContentInfo, FontManager, Rasterized};

static R_EMPTY: &[u8] = &[0; 9];
static R_FULL: &[u8] = &[255; 9];
static R_VERT: &[u8] = &[0, 0, 0, 0, 255, 255, 0, 0, 0];
static R_RIGHT: &[u8] = &[0, 0, 0, 0, 255, 255, 255, 255, 255];
static R_LEFT: &[u8] = &[255, 255, 255, 255, 255, 255, 0, 0, 0];
static R_HALF_L: &[u8] = &[255, 255, 255, 255, 255, 0, 0, 0, 0];
static R_HALF_R: &[u8] = &[0, 0, 0, 0, 0, 255, 255, 255, 255];

static R_SHADE_L1: &[u8] = &[255, 0, 0, 0, 255, 0, 0, 0, 255];
static R_SHADE_L2: &[u8] = &[0, 0, 255, 0, 0, 0, 255, 0, 0];
static R_SHADE_M1: &[u8] = &[255, 0, 255, 0, 255, 0, 255, 0, 255];
static R_SHADE_M2: &[u8] = &[0, 255, 0, 255, 0, 255, 0, 255, 0];
static R_SHADE_D1: &[u8] = &[255, 255, 255, 0, 255, 255, 255, 0, 255];
static R_SHADE_D2: &[u8] = &[255, 0, 255, 255, 255, 0, 255, 255, 255];

static GLYPH_HLINE: [&[u8]; 20] = [
    R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_FULL,
    R_FULL, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY,
];

static GLYPH_VLINE: [&[u8]; 20] = [
    R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT,
    R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT,
];

static GLYPH_TOP_LEFT: [&[u8]; 20] = [
    R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_RIGHT,
    R_RIGHT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT,
];

static GLYPH_TOP_RIGHT: [&[u8]; 20] = [
    R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_LEFT,
    R_LEFT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT,
];

static GLYPH_BOTTOM_LEFT: [&[u8]; 20] = [
    R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_RIGHT, R_RIGHT,
    R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY,
];

static GLYPH_BOTTOM_RIGHT: [&[u8]; 20] = [
    R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_LEFT, R_LEFT,
    R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY,
];

static GLYPH_T_LEFT: [&[u8]; 20] = [
    R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_RIGHT, R_RIGHT,
    R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT,
];

static GLYPH_T_RIGHT: [&[u8]; 20] = [
    R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_LEFT, R_LEFT, R_VERT,
    R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT,
];

static GLYPH_T_TOP: [&[u8]; 20] = [
    R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_FULL,
    R_FULL, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT,
];

static GLYPH_T_BOTTOM: [&[u8]; 20] = [
    R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_FULL, R_FULL,
    R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY,
];

static GLYPH_CROSS: [&[u8]; 20] = [
    R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_FULL, R_FULL, R_VERT,
    R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT, R_VERT,
];

static GLYPH_BLOCK_FULL: [&[u8]; 20] = [R_FULL; 20];

static GLYPH_BLOCK_UPPER: [&[u8]; 20] = [
    R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_EMPTY,
    R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY,
];

static GLYPH_BLOCK_LOWER: [&[u8]; 20] = [
    R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY,
    R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL,
];

static GLYPH_BLOCK_LEFT: [&[u8]; 20] = [R_HALF_L; 20];
static GLYPH_BLOCK_RIGHT: [&[u8]; 20] = [R_HALF_R; 20];

static GLYPH_BAR_1: [&[u8]; 20] = [
    R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY,
    R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_FULL, R_FULL, R_FULL,
];
static GLYPH_BAR_2: [&[u8]; 20] = [
    R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY,
    R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL,
];
static GLYPH_BAR_3: [&[u8]; 20] = [
    R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY,
    R_EMPTY, R_EMPTY, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL,
];
static GLYPH_BAR_5: [&[u8]; 20] = [
    R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_FULL, R_FULL, R_FULL, R_FULL,
    R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL,
];
static GLYPH_BAR_6: [&[u8]; 20] = [
    R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_EMPTY, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL,
    R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL,
];
static GLYPH_BAR_7: [&[u8]; 20] = [
    R_EMPTY, R_EMPTY, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL,
    R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL, R_FULL,
];

static GLYPH_SHADE_LIGHT: [&[u8]; 20] = [
    R_SHADE_L1, R_SHADE_L2, R_SHADE_L1, R_SHADE_L2, R_SHADE_L1, R_SHADE_L2, R_SHADE_L1, R_SHADE_L2,
    R_SHADE_L1, R_SHADE_L2, R_SHADE_L1, R_SHADE_L2, R_SHADE_L1, R_SHADE_L2, R_SHADE_L1, R_SHADE_L2,
    R_SHADE_L1, R_SHADE_L2, R_SHADE_L1, R_SHADE_L2,
];

static GLYPH_SHADE_MEDIUM: [&[u8]; 20] = [
    R_SHADE_M1, R_SHADE_M2, R_SHADE_M1, R_SHADE_M2, R_SHADE_M1, R_SHADE_M2, R_SHADE_M1, R_SHADE_M2,
    R_SHADE_M1, R_SHADE_M2, R_SHADE_M1, R_SHADE_M2, R_SHADE_M1, R_SHADE_M2, R_SHADE_M1, R_SHADE_M2,
    R_SHADE_M1, R_SHADE_M2, R_SHADE_M1, R_SHADE_M2,
];

static GLYPH_SHADE_DARK: [&[u8]; 20] = [
    R_SHADE_D1, R_SHADE_D2, R_SHADE_D1, R_SHADE_D2, R_SHADE_D1, R_SHADE_D2, R_SHADE_D1, R_SHADE_D2,
    R_SHADE_D1, R_SHADE_D2, R_SHADE_D1, R_SHADE_D2, R_SHADE_D1, R_SHADE_D2, R_SHADE_D1, R_SHADE_D2,
    R_SHADE_D1, R_SHADE_D2, R_SHADE_D1, R_SHADE_D2,
];

fn get_custom_glyph(c: char) -> Option<&'static [&'static [u8]]> {
    match c {
        '─' | '━' | '═' | '┄' | '┅' | '┈' | '┉' | '╌' | '╍' => Some(&GLYPH_HLINE),
        '│' | '┃' | '║' | '┆' | '┇' | '┊' | '┋' | '╎' | '╏' => Some(&GLYPH_VLINE),
        '┌' | '┍' | '┎' | '┏' | '╔' | '╒' | '╓' | '╭' => Some(&GLYPH_TOP_LEFT),
        '┐' | '┑' | '┒' | '┓' | '╗' | '╕' | '╖' | '╮' => Some(&GLYPH_TOP_RIGHT),
        '└' | '┕' | '┖' | '┗' | '╚' | '╘' | '╙' | '╰' => Some(&GLYPH_BOTTOM_LEFT),
        '┘' | '┙' | '┚' | '┛' | '╝' | '╛' | '╜' | '╯' => Some(&GLYPH_BOTTOM_RIGHT),
        '├' | '┝' | '┞' | '┟' | '┠' | '┡' | '┢' | '┣' | '╠' | '╞' | '╟' => {
            Some(&GLYPH_T_LEFT)
        }
        '┤' | '┥' | '┦' | '┧' | '┨' | '┩' | '┪' | '┫' | '╣' | '╡' | '╢' => {
            Some(&GLYPH_T_RIGHT)
        }
        '┬' | '┭' | '┮' | '┯' | '┰' | '┱' | '┲' | '┳' | '╦' | '╤' | '╥' => {
            Some(&GLYPH_T_TOP)
        }
        '┴' | '┵' | '┶' | '┷' | '┸' | '┹' | '┺' | '┻' | '╩' | '╧' | '╨' => {
            Some(&GLYPH_T_BOTTOM)
        }
        '┼' | '┽' | '┾' | '┿' | '╀' | '╁' | '╂' | '╃' | '╄' | '╅' | '╆' | '╇' | '╈' | '╉' | '╊'
        | '╋' | '╬' | '╪' | '╫' => Some(&GLYPH_CROSS),
        '█' => Some(&GLYPH_BLOCK_FULL),
        '▀' => Some(&GLYPH_BLOCK_UPPER),
        '▄' => Some(&GLYPH_BLOCK_LOWER),
        '▌' => Some(&GLYPH_BLOCK_LEFT),
        '▐' => Some(&GLYPH_BLOCK_RIGHT),
        '▁' => Some(&GLYPH_BAR_1),
        '▂' => Some(&GLYPH_BAR_2),
        '▃' => Some(&GLYPH_BAR_3),
        '▅' => Some(&GLYPH_BAR_5),
        '▆' => Some(&GLYPH_BAR_6),
        '▇' => Some(&GLYPH_BAR_7),
        '░' => Some(&GLYPH_SHADE_LIGHT),
        '▒' => Some(&GLYPH_SHADE_MEDIUM),
        '▓' => Some(&GLYPH_SHADE_DARK),
        _ => None,
    }
}

fn map_unicode_to_ascii(c: char) -> char {
    match c {
        '↑' | '▲' | '▴' => '^',
        '↓' | '▼' | '▾' => 'v',
        '←' | '◀' | '◄' => '<',
        '→' | '▶' | '►' => '>',
        '↔' => '-',
        '⁰' => '0',
        '¹' => '1',
        '²' => '2',
        '³' => '3',
        '⁴' => '4',
        '⁵' => '5',
        '⁶' => '6',
        '⁷' => '7',
        '⁸' => '8',
        '⁹' => '9',
        '₀' => '0',
        '₁' => '1',
        '₂' => '2',
        '₃' => '3',
        '₄' => '4',
        '₅' => '5',
        '₆' => '6',
        '₇' => '7',
        '₈' => '8',
        '₉' => '9',
        '•' | '●' | '○' | '■' | '□' | '▪' | '▫' | '◆' | '◇' | '◈' | '♦' => {
            '*'
        }
        '–' | '—' | '−' | '―' => '-',
        '…' | '·' => '.',
        '‘' | '’' | '‚' | '‛' => '\'',
        '“' | '”' | '„' | '‟' => '"',
        '«' | '‹' => '<',
        '»' | '›' => '>',
        '°' => 'o',
        '±' => '+',
        '×' => 'x',
        '÷' => '/',
        '\u{00A0}' => ' ',
        '\u{2800}' => ' ',
        '\u{2801}'..='\u{28FF}' => ':',
        '▘' | '▝' | '▖' | '▗' | '▚' | '▞' | '▛' | '▜' | '▙' | '▟' => '#',
        _ => c,
    }
}

/// Enhanced bitmap font with box-drawing, block elements, and Unicode fallback.
pub struct ConsoleFont {
    inner: BitmapFont,
}

impl ConsoleFont {
    pub const fn new() -> Self {
        Self { inner: BitmapFont }
    }
}

impl Default for ConsoleFont {
    fn default() -> Self {
        Self::new()
    }
}

impl FontManager for ConsoleFont {
    fn size(&self) -> (usize, usize) {
        self.inner.size()
    }

    fn rasterize(&mut self, mut info: ContentInfo) -> Rasterized<'_> {
        if let Some(glyph) = get_custom_glyph(info.content) {
            return Rasterized::GraySlice(glyph);
        }
        info.content = map_unicode_to_ascii(info.content);
        self.inner.rasterize(info)
    }
}
