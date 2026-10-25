//! A capture and what was drawn on it, with undo and redo.

use super::{Annotation, Point};
use crate::geometry::{PixelPoint, PixelRect};
use crate::pixels::PixelBuffer;
use crate::uikit::painter::Fonts;
use std::sync::Arc;

/// One step of the history.
#[derive(Clone, Debug, PartialEq)]
enum Step {
    Add(Annotation),
    Crop { before: Option<PixelRect>, after: PixelRect },
}

pub struct Document {
    base: Arc<PixelBuffer>,
    annotations: Vec<Annotation>,
    /// In image pixels; `None` shows the whole capture.
    crop: Option<PixelRect>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    version: u64,
    saved_version: u64,
}

impl Document {
    /// Coordinates are image pixels from the capture's top-left corner,
    /// wherever it was on the screen.
    pub fn new(base: Arc<PixelBuffer>) -> Document {
        Document {
            base,
            annotations: Vec::new(),
            crop: None,
            undo: Vec::new(),
            redo: Vec::new(),
            version: 0,
            saved_version: 0,
        }
    }

    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    pub fn image_rect(&self) -> PixelRect {
        PixelRect::new(0, 0, self.base.width(), self.base.height())
    }

    pub fn add(&mut self, a: Annotation) {
        self.apply(&Step::Add(a.clone()));
        self.undo.push(Step::Add(a));
        self.redo.clear();
    }

    /// Crops to the rectangle between `a` and `b`, within what is visible;
    /// anything under 4×4 pixels is ignored.
    pub fn crop_to(&mut self, a: Point, b: Point) {
        let corner = |p: Point| PixelPoint { x: p.0.round() as i32, y: p.1.round() as i32 };
        let rect = PixelRect::from_points(corner(a), corner(b)).intersect(&self.visible_rect());
        if rect.width < 4 || rect.height < 4 {
            return;
        }
        let step = Step::Crop { before: self.crop, after: rect };
        self.apply(&step);
        self.undo.push(step);
        self.redo.clear();
    }

    fn apply(&mut self, step: &Step) {
        match step {
            Step::Add(a) => self.annotations.push(a.clone()),
            Step::Crop { after, .. } => self.crop = Some(*after),
        }
        self.version += 1;
    }

    pub fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop() else { return false };
        match &step {
            Step::Add(_) => {
                self.annotations.pop();
            }
            Step::Crop { before, .. } => self.crop = *before,
        }
        self.redo.push(step);
        self.version += 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else { return false };
        self.apply(&step);
        self.undo.push(step);
        true
    }

    /// Changes on every edit, undo and redo, and on nothing else.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn is_dirty(&self) -> bool {
        self.version != self.saved_version
    }

    pub fn mark_saved(&mut self) {
        self.saved_version = self.version;
    }

    pub fn visible_rect(&self) -> PixelRect {
        self.crop.unwrap_or_else(|| self.image_rect())
    }

    /// The capture with every annotation drawn in order, cropped.
    pub fn flatten(&self, fonts: &Fonts) -> PixelBuffer {
        super::draw::flatten(&self.base, &self.annotations, self.visible_rect(), fonts)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{PALETTE, Rgb};
    use super::*;

    fn doc(w: i32, h: i32) -> Document {
        Document::new(Arc::new(PixelBuffer::solid(PixelRect::new(300, 200, w, h), 255, 255, 255)))
    }

    fn rect(n: f32) -> Annotation {
        Annotation::Rectangle { a: (n, n), b: (n + 10.0, n + 10.0), color: PALETTE[0], width: 3.0 }
    }

    #[test]
    fn undo_and_redo_walk_the_history() {
        let mut d = doc(100, 80);
        d.add(rect(1.0));
        d.add(rect(2.0));
        d.crop_to((10.0, 10.0), (60.0, 50.0));
        assert_eq!(d.visible_rect(), PixelRect::new(10, 10, 50, 40));
        assert!(d.undo() && d.undo() && d.undo());
        assert!(!d.undo());
        assert!(d.annotations().is_empty());
        assert_eq!(d.visible_rect(), PixelRect::new(0, 0, 100, 80));
        assert!(d.redo() && d.redo());
        assert_eq!(d.annotations(), [rect(1.0), rect(2.0)]);
        d.add(rect(3.0));
        assert!(!d.redo(), "a new edit clears redo");
    }

    #[test]
    fn document_version_changes_only_on_edits() {
        let mut d = doc(100, 80);
        let fonts = Fonts::embedded();
        let v0 = d.version();
        let _ = (d.visible_rect(), d.flatten(&fonts), d.is_dirty());
        assert_eq!(d.version(), v0);
        d.add(rect(1.0));
        assert_eq!(d.version(), v0 + 1);
        d.undo();
        d.redo();
        d.crop_to((0.0, 0.0), (50.0, 50.0));
        assert_eq!(d.version(), v0 + 4);
        assert!(!d.redo());
        assert_eq!(d.version(), v0 + 4, "nothing to redo changes nothing");
    }

    #[test]
    fn dirty_until_saved() {
        let mut d = doc(100, 80);
        assert!(!d.is_dirty());
        d.add(rect(1.0));
        assert!(d.is_dirty());
        d.mark_saved();
        assert!(!d.is_dirty());
        d.undo();
        assert!(d.is_dirty());
    }

    #[test]
    fn crop_smaller_than_four_pixels_is_ignored() {
        let mut d = doc(100, 80);
        let v = d.version();
        d.crop_to((10.0, 10.0), (12.0, 12.0));
        assert_eq!((d.version(), d.visible_rect()), (v, PixelRect::new(0, 0, 100, 80)));
    }

    #[test]
    fn crop_is_clamped_and_flatten_has_its_size() {
        let mut d = doc(100, 80);
        d.crop_to((-20.0, -20.0), (50.0, 40.0));
        let img = d.flatten(&Fonts::embedded());
        assert_eq!(img.bounds, PixelRect::new(0, 0, 50, 40));
        d.crop_to((40.0, 30.0), (200.0, 200.0));
        assert_eq!(d.visible_rect(), PixelRect::new(40, 30, 10, 10), "crop within the crop");
        let _ = Rgb(0, 0, 0);
    }
}
