//! The annotation editor's model: what was drawn on a capture, in image
//! pixels, with undo, cropping and the final picture.

mod document;
mod draw;
mod view;

pub use document::Document;
pub use draw::{draw, load, save};
pub use view::View;

use crate::uikit::theme::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Arrow,
    Rectangle,
    Pen,
    Highlighter,
    Text,
    Pixelate,
    /// A solid box over what must not be seen.
    Redact,
    Crop,
}

pub const TOOLS: [Tool; 8] = [
    Tool::Arrow,
    Tool::Rectangle,
    Tool::Pen,
    Tool::Highlighter,
    Tool::Text,
    Tool::Pixelate,
    Tool::Redact,
    Tool::Crop,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Thin,
    Medium,
    Thick,
}

pub const SIZES: [Size; 3] = [Size::Thin, Size::Medium, Size::Thick];

impl Size {
    /// Line width in image pixels.
    pub fn stroke(self) -> f32 {
        match self {
            Size::Thin => 3.0,
            Size::Medium => 6.0,
            Size::Thick => 12.0,
        }
    }

    /// Text height in image pixels.
    pub fn text_px(self) -> f32 {
        match self {
            Size::Thin => 18.0,
            Size::Medium => 28.0,
            Size::Thick => 44.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn color(self) -> Color {
        Color(u32::from(self.0) << 16 | u32::from(self.1) << 8 | u32::from(self.2))
    }
}

/// Red, yellow, green, blue, black, white.
pub const PALETTE: [Rgb; 6] = [
    Rgb(0xE5, 0x39, 0x35),
    Rgb(0xFD, 0xD8, 0x35),
    Rgb(0x43, 0xA0, 0x47),
    Rgb(0x1E, 0x88, 0xE5),
    Rgb(0x00, 0x00, 0x00),
    Rgb(0xFF, 0xFF, 0xFF),
];

/// A point in image pixels, on the whole (uncropped) capture.
pub type Point = (f32, f32);

#[derive(Clone, Debug, PartialEq)]
pub enum Annotation {
    Arrow {
        from: Point,
        to: Point,
        color: Rgb,
        width: f32,
    },
    Rectangle {
        a: Point,
        b: Point,
        color: Rgb,
        width: f32,
    },
    /// Free-hand line; a highlighter stroke is wide and see-through.
    Stroke {
        points: Vec<Point>,
        color: Rgb,
        width: f32,
        highlight: bool,
    },
    /// `at` is the top-left corner of the text.
    Text {
        at: Point,
        text: String,
        color: Rgb,
        px: f32,
    },
    /// Blocks of `block` pixels, each one the average of what is below it.
    Pixelate {
        a: Point,
        b: Point,
        block: f32,
    },
    /// The area filled with `color`.
    Redact {
        a: Point,
        b: Point,
        color: Rgb,
    },
}

/// Pixelate block for a capture: coarse enough to hide text at any size
/// when thin, twice and four times as coarse when medium and thick.
pub fn pixel_block(width: i32, height: i32, size: Size) -> f32 {
    let coarse = match size {
        Size::Thin => 1.0,
        Size::Medium => 2.0,
        Size::Thick => 4.0,
    };
    (width.max(height) as f32 / 120.0).max(8.0).round() * coarse
}
