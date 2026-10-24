//! Colors and metrics of the "gyoza" look.

/// `0x00RRGGBB`, the softbuffer pixel format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    pub fn rgb(self) -> (u8, u8, u8) {
        ((self.0 >> 16) as u8, (self.0 >> 8) as u8, self.0 as u8)
    }
}

/// Lengths are logical units (96-dpi pixels).
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub bg: Color,
    pub button: Color,
    pub button_hover: Color,
    pub button_pressed: Color,
    pub text: Color,
    pub disabled_text: Color,
    pub accent: Color,
    pub error: Color,
    pub radius: f32,
    pub font_px: f32,
    pub padding: f32,
    pub control_height: f32,
}

pub const GYOZA: Theme = Theme {
    bg: Color(0xFFF6E5),
    button: Color(0xFFE3B3),
    button_hover: Color(0xFFD28A),
    button_pressed: Color(0xF5C06A),
    text: Color(0x4A2C1A),
    disabled_text: Color(0xA08C78),
    accent: Color(0xE0533C),
    error: Color(0xC62828),
    radius: 6.0,
    font_px: 14.0,
    padding: 8.0,
    control_height: 28.0,
};
