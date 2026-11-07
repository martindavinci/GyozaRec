// SPDX-License-Identifier: GPL-3.0-or-later
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
    Crop {
        before: Option<PixelRect>,
        after: PixelRect,
    },
    /// A quarter turn: the drawings become part of the turned picture.
    Rotate {
        before: Box<Picture>,
        after: Box<Picture>,
    },
}

/// Everything a rotation changes.
#[derive(Clone, Debug, PartialEq)]
struct Picture {
    base: Arc<PixelBuffer>,
    annotations: Vec<Annotation>,
    crop: Option<PixelRect>,
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

    /// Turns the picture a quarter turn; what was drawn turns with it and
    /// can no longer be undone one by one until the turn is undone.
    pub fn rotate(&mut self, clockwise: bool, fonts: &Fonts) {
        let full = self.image_rect();
        let drawn = super::draw::flatten(&self.base, &self.annotations, full, fonts);
        let after = Picture {
            base: Arc::new(drawn.rotated(clockwise)),
            annotations: Vec::new(),
            crop: self.crop.map(|c| rotate_rect(c, full, clockwise)),
        };
        let step = Step::Rotate { before: Box::new(self.picture()), after: Box::new(after) };
        self.apply(&step);
        self.undo.push(step);
        self.redo.clear();
    }

    fn picture(&self) -> Picture {
        Picture { base: self.base.clone(), annotations: self.annotations.clone(), crop: self.crop }
    }

    fn set_picture(&mut self, p: &Picture) {
        self.base = p.base.clone();
        self.annotations = p.annotations.clone();
        self.crop = p.crop;
    }

    fn apply(&mut self, step: &Step) {
        match step {
            Step::Add(a) => self.annotations.push(a.clone()),
            Step::Crop { after, .. } => self.crop = Some(*after),
            Step::Rotate { after, .. } => self.set_picture(after),
        }
        self.version += 1;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop() else { return false };
        match &step {
            Step::Add(_) => {
                self.annotations.pop();
            }
            Step::Crop { before, .. } => self.crop = *before,
            Step::Rotate { before, .. } => self.set_picture(before),
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

    /// The picture of `version` reached the disk (edits made meanwhile stay unsaved).
    pub fn mark_saved_at(&mut self, version: u64) {
        self.saved_version = version;
    }

    /// A copy sharing the capture, to draw the final picture elsewhere.
    pub fn snapshot(&self) -> Document {
        Document {
            base: self.base.clone(),
            annotations: self.annotations.clone(),
            crop: self.crop,
            undo: Vec::new(),
            redo: Vec::new(),
            version: self.version,
            saved_version: self.saved_version,
        }
    }

    pub fn visible_rect(&self) -> PixelRect {
        self.crop.unwrap_or_else(|| self.image_rect())
    }

    /// The capture with every annotation drawn in order, cropped.
    pub fn flatten(&self, fonts: &Fonts) -> PixelBuffer {
        super::draw::flatten(&self.base, &self.annotations, self.visible_rect(), fonts)
    }
}

/// `r`, inside a `full` picture at (0, 0), after a quarter turn.
fn rotate_rect(r: PixelRect, full: PixelRect, clockwise: bool) -> PixelRect {
    if clockwise {
        PixelRect::new(full.height - r.y - r.height, r.x, r.height, r.width)
    } else {
        PixelRect::new(r.y, full.width - r.x - r.width, r.height, r.width)
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
    fn edits_during_a_save_stay_unsaved() {
        let mut d = doc(100, 80);
        d.add(rect(1.0));
        let saving = d.version();
        d.add(rect(2.0));
        d.mark_saved_at(saving);
        assert!(d.is_dirty());
        assert_eq!(d.snapshot().annotations(), d.annotations());
    }

    #[test]
    fn rotation_turns_picture_drawings_and_crop_and_undoes() {
        let fonts = Fonts::embedded();
        let mut d = doc(100, 80);
        d.add(Annotation::Redact { a: (0.0, 0.0), b: (10.0, 10.0), color: PALETTE[4] });
        d.crop_to((0.0, 0.0), (50.0, 40.0));
        let before = d.flatten(&fonts);
        d.rotate(true, &fonts);
        assert_eq!(d.image_rect(), PixelRect::new(0, 0, 80, 100));
        assert!(d.annotations().is_empty(), "the drawings are part of the picture now");
        assert_eq!(d.visible_rect(), PixelRect::new(40, 0, 40, 50));
        let turned = d.flatten(&fonts);
        assert_eq!(turned, before.rotated(true), "the same picture, turned");
        assert_eq!(turned.pixel(PixelPoint { x: 35, y: 2 }), [0, 0, 0, 255], "the box went right");
        d.rotate(false, &fonts);
        assert_eq!(d.flatten(&fonts), before);
        assert!(d.undo() && d.undo());
        assert_eq!(d.annotations().len(), 1);
        assert_eq!(d.visible_rect(), PixelRect::new(0, 0, 50, 40));
        assert_eq!(d.image_rect(), PixelRect::new(0, 0, 100, 80));
        assert!(d.redo());
        assert_eq!(d.flatten(&fonts), turned);
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
