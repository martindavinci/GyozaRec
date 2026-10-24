//! A handful of widgets laid out in rows, with hover/press tracking.

use super::painter::{Fonts, Painter, RectF, measure_text};
use super::theme::{Color, Theme};

const CHECK_BOX: f32 = 16.0;
const CHECK_GAP: f32 = 6.0;
const DROPDOWN_EXTRA: f32 = 16.0;

#[derive(Clone, Debug, PartialEq)]
pub enum WidgetKind {
    Button { label: String, dropdown: bool },
    Check { label: String, checked: bool },
    Label { text: String, color: Color },
    Link { text: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Widget {
    pub id: u32,
    pub kind: WidgetKind,
    pub rect: RectF,
    pub enabled: bool,
}

fn natural_size(kind: &WidgetKind, fonts: &Fonts, theme: &Theme) -> (f32, f32) {
    let px = theme.font_px;
    match kind {
        WidgetKind::Button { label, dropdown } => {
            let (w, _) = measure_text(fonts, label, px, false);
            let extra = if *dropdown { DROPDOWN_EXTRA } else { 0.0 };
            (w + 2.0 * theme.padding + extra, theme.control_height)
        }
        WidgetKind::Check { label, .. } => {
            let (w, h) = measure_text(fonts, label, px, false);
            (CHECK_BOX + CHECK_GAP + w, h.max(CHECK_BOX))
        }
        WidgetKind::Label { text, .. } | WidgetKind::Link { text } => {
            measure_text(fonts, text, px, false)
        }
    }
}

/// Lays widgets left to right from (`x`, `y`), vertically centered in the row.
pub fn layout_row(
    items: Vec<(u32, WidgetKind)>,
    x: f32,
    y: f32,
    gap: f32,
    fonts: &Fonts,
    theme: &Theme,
) -> Vec<Widget> {
    let sized: Vec<_> = items
        .into_iter()
        .map(|(id, kind)| {
            let size = natural_size(&kind, fonts, theme);
            (id, kind, size)
        })
        .collect();
    let row_h = sized.iter().map(|(_, _, (_, h))| *h).fold(0.0, f32::max);
    let mut cursor = x;
    sized
        .into_iter()
        .map(|(id, kind, (w, h))| {
            let rect = RectF { x: cursor, y: y + (row_h - h) / 2.0, w, h };
            cursor += w + gap;
            Widget { id, kind, rect, enabled: true }
        })
        .collect()
}

pub struct Ui {
    pub widgets: Vec<Widget>,
    hover: Option<u32>,
    pressed: Option<u32>,
}

impl Ui {
    pub fn new(widgets: Vec<Widget>) -> Ui {
        Ui { widgets, hover: None, pressed: None }
    }

    pub fn widget_mut(&mut self, id: u32) -> Option<&mut Widget> {
        self.widgets.iter_mut().find(|w| w.id == id)
    }

    pub fn widget(&self, id: u32) -> Option<&Widget> {
        self.widgets.iter().find(|w| w.id == id)
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<u32> {
        self.widgets.iter().find(|w| w.enabled && w.rect.contains(x, y)).map(|w| w.id)
    }

    /// Returns whether the hover state changed (redraw needed).
    pub fn pointer_moved(&mut self, x: f32, y: f32) -> bool {
        let hover = self.hit_test(x, y);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    pub fn pointer_down(&mut self, x: f32, y: f32) {
        self.pressed = self.hit_test(x, y);
    }

    /// A click is a press and a release on the same enabled widget.
    pub fn pointer_up(&mut self, x: f32, y: f32) -> Option<u32> {
        let released = self.hit_test(x, y);
        let pressed = self.pressed.take();
        if pressed.is_some() && pressed == released { pressed } else { None }
    }

    pub fn pointer_left(&mut self) -> bool {
        let changed = self.hover.is_some();
        self.hover = None;
        changed
    }

    /// Union of all widget rects.
    pub fn bounds(&self) -> RectF {
        let Some(first) = self.widgets.first() else { return RectF::default() };
        let mut r = first.rect;
        for w in &self.widgets[1..] {
            let right = r.right().max(w.rect.right());
            let bottom = r.bottom().max(w.rect.bottom());
            r.x = r.x.min(w.rect.x);
            r.y = r.y.min(w.rect.y);
            r.w = right - r.x;
            r.h = bottom - r.y;
        }
        r
    }

    pub fn paint(&self, p: &mut Painter, fonts: &Fonts, theme: &Theme) {
        for w in &self.widgets {
            let text_color = if w.enabled { theme.text } else { theme.disabled_text };
            let px = theme.font_px;
            match &w.kind {
                WidgetKind::Button { label, dropdown } => {
                    let fill = if !w.enabled {
                        theme.button
                    } else if self.pressed == Some(w.id) && self.hover == Some(w.id) {
                        theme.button_pressed
                    } else if self.hover == Some(w.id) {
                        theme.button_hover
                    } else {
                        theme.button
                    };
                    p.fill_rounded_rect(w.rect, theme.radius, fill);
                    let (tw, th) = measure_text(fonts, label, px, false);
                    let extra = if *dropdown { DROPDOWN_EXTRA } else { 0.0 };
                    let tx = w.rect.x + (w.rect.w - extra - tw) / 2.0;
                    let ty = w.rect.y + (w.rect.h - th) / 2.0;
                    p.text(fonts, label, tx, ty, px, text_color, false);
                    if *dropdown {
                        let cx = w.rect.right() - theme.padding - 4.0;
                        let cy = w.rect.y + w.rect.h / 2.0;
                        p.fill_triangle(
                            [(cx - 4.0, cy - 2.0), (cx + 4.0, cy - 2.0), (cx, cy + 3.0)],
                            text_color,
                        );
                    }
                }
                WidgetKind::Check { label, checked } => {
                    let by = w.rect.y + (w.rect.h - CHECK_BOX) / 2.0;
                    let outer = RectF { x: w.rect.x, y: by, w: CHECK_BOX, h: CHECK_BOX };
                    p.fill_rounded_rect(outer, 3.0, text_color);
                    let inner = RectF { x: outer.x + 1.5, y: by + 1.5, w: 13.0, h: 13.0 };
                    p.fill_rounded_rect(inner, 2.0, Color(0xFFFFFF));
                    if *checked {
                        let mark = RectF { x: outer.x + 4.0, y: by + 4.0, w: 8.0, h: 8.0 };
                        p.fill_rounded_rect(mark, 2.0, theme.accent);
                    }
                    let (_, th) = measure_text(fonts, label, px, false);
                    let tx = w.rect.x + CHECK_BOX + CHECK_GAP;
                    p.text(
                        fonts,
                        label,
                        tx,
                        w.rect.y + (w.rect.h - th) / 2.0,
                        px,
                        text_color,
                        false,
                    );
                }
                WidgetKind::Label { text, color } => {
                    p.text(fonts, text, w.rect.x, w.rect.y, px, *color, false);
                }
                WidgetKind::Link { text } => {
                    p.text(fonts, text, w.rect.x, w.rect.y, px, theme.accent, false);
                    let line = RectF { x: w.rect.x, y: w.rect.bottom() - 2.0, w: w.rect.w, h: 1.0 };
                    p.fill_rounded_rect(line, 0.0, theme.accent);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uikit::painter::Fonts;
    use crate::uikit::theme::GYOZA;

    fn btn(l: &str) -> WidgetKind {
        WidgetKind::Button { label: l.into(), dropdown: false }
    }

    #[test]
    fn layout_row_places_widgets_with_gap() {
        let f = Fonts::embedded();
        let ws =
            layout_row(vec![(1, btn("Capture")), (2, btn("Options"))], 6.0, 6.0, 6.0, &f, &GYOZA);
        assert_eq!(ws[1].rect.x, ws[0].rect.right() + 6.0);
        assert_eq!(ws[0].rect.h, 28.0);
    }

    #[test]
    fn hit_test_and_click_need_same_widget() {
        let f = Fonts::embedded();
        let mut ui =
            Ui::new(layout_row(vec![(1, btn("A")), (2, btn("B"))], 0.0, 0.0, 10.0, &f, &GYOZA));
        let (a, b) = (ui.widgets[0].rect, ui.widgets[1].rect);
        assert_eq!(ui.hit_test(a.x + 1.0, a.y + 1.0), Some(1));
        assert_eq!(ui.hit_test(a.right() + 5.0, a.y + 1.0), None);
        ui.pointer_down(a.x + 1.0, a.y + 1.0);
        assert_eq!(ui.pointer_up(b.x + 1.0, b.y + 1.0), None);
        ui.pointer_down(a.x + 1.0, a.y + 1.0);
        assert_eq!(ui.pointer_up(a.x + 2.0, a.y + 2.0), Some(1));
    }

    #[test]
    fn disabled_widget_is_not_hit() {
        let f = Fonts::embedded();
        let mut ws = layout_row(vec![(1, btn("A"))], 0.0, 0.0, 0.0, &f, &GYOZA);
        ws[0].enabled = false;
        let ui = Ui::new(ws);
        assert_eq!(ui.hit_test(1.0, 1.0), None);
    }

    #[test]
    fn hover_change_requests_redraw() {
        let f = Fonts::embedded();
        let mut ui = Ui::new(layout_row(vec![(1, btn("A"))], 0.0, 0.0, 0.0, &f, &GYOZA));
        assert!(ui.pointer_moved(1.0, 1.0));
        assert!(!ui.pointer_moved(2.0, 2.0));
        assert!(ui.pointer_moved(500.0, 500.0));
    }
}
