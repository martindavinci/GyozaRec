//! Colors and metrics of the "gyoza" look, light and dark.

use std::sync::atomic::{AtomicU8, Ordering};

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
    /// Behind the picture in the editor.
    pub canvas: Color,
    /// Inside a checkbox.
    pub check_well: Color,
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
    canvas: Color(0xEADBC4),
    check_well: Color(0xFFFFFF),
    radius: 6.0,
    font_px: 14.0,
    padding: 8.0,
    control_height: 28.0,
};

/// The same look after dark: a toasted crust instead of raw dough.
pub const GYOZA_DARK: Theme = Theme {
    bg: Color(0x2B221D),
    button: Color(0x4A3A2E),
    button_hover: Color(0x5E4A3A),
    button_pressed: Color(0x725944),
    text: Color(0xF4E6D4),
    disabled_text: Color(0x9A8877),
    accent: Color(0xF0705A),
    error: Color(0xFF7A70),
    canvas: Color(0x1C1612),
    check_well: Color(0x2B221D),
    ..GYOZA
};

/// Cool blue-grey dark, from a palette a user shared.
pub const SLATE: Theme = Theme {
    bg: Color(0x1E2026),
    button: Color(0x2B303A),
    button_hover: Color(0x383F4B),
    button_pressed: Color(0x464F60),
    text: Color(0xE8EDF3),
    disabled_text: Color(0xAAB4C2),
    accent: Color(0x85B7FF),
    error: Color(0xFF8C8C),
    canvas: Color(0x14171C),
    check_well: Color(0x15181D),
    ..GYOZA
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Light,
    Dark,
    Slate,
}

impl Look {
    const ALL: [Look; 3] = [Look::Light, Look::Dark, Look::Slate];

    pub fn theme(self) -> &'static Theme {
        match self {
            Look::Light => &GYOZA,
            Look::Dark => &GYOZA_DARK,
            Look::Slate => &SLATE,
        }
    }

    /// Dark title bars go with it.
    pub fn is_dark(self) -> bool {
        self != Look::Light
    }
}

static LOOK: AtomicU8 = AtomicU8::new(0);

/// Switches every window drawn from now on to `look`.
pub fn set_look(look: Look) {
    LOOK.store(look as u8, Ordering::Relaxed);
}

pub fn look() -> Look {
    Look::ALL[usize::from(LOOK.load(Ordering::Relaxed)).min(Look::ALL.len() - 1)]
}

/// The look in use.
pub fn current() -> &'static Theme {
    look().theme()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG contrast ratio of two colors.
    fn contrast(a: Color, b: Color) -> f32 {
        let lum = |c: Color| {
            let (r, g, b) = c.rgb();
            let lin = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn text_is_readable_in_both_looks() {
        for t in [GYOZA, GYOZA_DARK, SLATE] {
            for bg in [t.bg, t.button, t.button_hover, t.button_pressed] {
                assert!(contrast(t.text, bg) >= 4.5, "{:?} on {:?}", t.text, bg);
            }
            assert!(contrast(t.accent, t.bg) >= 3.0, "{:?}", t.accent);
            assert!(contrast(t.disabled_text, t.bg) >= 2.5, "{:?}", t.disabled_text);
        }
    }

    #[test]
    fn each_look_has_its_colors() {
        let bgs: Vec<Color> = Look::ALL.iter().map(|l| l.theme().bg).collect();
        assert_eq!(bgs, [GYOZA.bg, GYOZA_DARK.bg, SLATE.bg]);
        assert_eq!(Look::ALL.map(Look::is_dark), [false, true, true]);
        assert_eq!(SLATE.control_height, GYOZA.control_height, "same sizes");
    }
}
