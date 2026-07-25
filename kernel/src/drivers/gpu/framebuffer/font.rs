use font8x8::UnicodeFonts;

/// Height of each character in pixels.
pub const FONT_HEIGHT: usize = 8;
/// Width of each character in pixels.
pub const FONT_WIDTH: usize = 8;

/// Retrieves the 8x8 font bitmap byte array for a given character.
///
/// Returns `Some([u8; 8])` if the character is supported in `font8x8::BASIC_FONTS`,
/// otherwise returns `None`.
pub fn get_char_bitmap(ch: char) -> Option<[u8; 8]> {
    font8x8::BASIC_FONTS.get(ch)
}
