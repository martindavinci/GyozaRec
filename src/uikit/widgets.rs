//! A handful of widgets laid out in rows, with hover/press tracking.

use super::painter::{Fonts, Painter, RectF, measure_text};
use super::theme::{Color, Theme};

const CHECK_BOX: f32 = 16.0;
const CHECK_GAP: f32 = 6.0;
const DROPDOWN_EXTRA: f32 = 16.0;

/// Symbols drawn as vector shapes, so they stay sharp at any scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Pause,
    Resume,
    Stop,
    Cancel,
    /// A bin: throws the recording away.
    Discard,
    /// A curved arrow: go back without doing anything.
    Back,
    Arrow,
    Rectangle,
    Pen,
    Highlighter,
    Text,
    Pixelate,
    /// A solid black bar.
    Redact,
    Crop,
    Undo,
    Redo,
    /// A window with its title bar.
    Window,
    /// Four corners: the whole screen.
    FullScreen,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WidgetKind {
    Button {
        label: String,
        dropdown: bool,
    },
    /// A square button showing only an icon.
    IconButton {
        icon: Icon,
    },
    /// An icon button that stays highlighted while its tool is chosen.
    ToolButton {
        icon: Icon,
        selected: bool,
    },
    /// A color to pick.
    Swatch {
        color: Color,
        selected: bool,
    },
    Check {
        label: String,
        checked: bool,
    },
    Label {
        text: String,
        color: Color,
    },
    Link {
        text: String,
    },
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
        WidgetKind::IconButton { .. }
        | WidgetKind::ToolButton { .. }
        | WidgetKind::Swatch { .. } => (theme.control_height, theme.control_height),
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

    /// Keeps the hover and press of `old` for widgets that still exist, so a
    /// rebuild between press and release does not lose the click.
    pub fn carry_state_from(&mut self, old: &Ui) {
        let exists = |id: Option<u32>| id.filter(|id| self.widget(*id).is_some());
        (self.hover, self.pressed) = (exists(old.hover), exists(old.pressed));
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
            let fill = if !w.enabled {
                theme.button
            } else if self.pressed == Some(w.id) && self.hover == Some(w.id) {
                theme.button_pressed
            } else if self.hover == Some(w.id) {
                theme.button_hover
            } else {
                theme.button
            };
            match &w.kind {
                WidgetKind::IconButton { icon } => {
                    p.fill_rounded_rect(w.rect, theme.radius, fill);
                    let accent = if w.enabled { theme.accent } else { text_color };
                    paint_icon(p, *icon, w.rect, text_color, accent);
                }
                WidgetKind::ToolButton { icon, selected } => {
                    if *selected {
                        p.fill_rounded_rect(w.rect, theme.radius, theme.accent);
                        p.fill_rounded_rect(
                            inset(w.rect, 2.0),
                            theme.radius - 1.0,
                            theme.button_pressed,
                        );
                    } else {
                        p.fill_rounded_rect(w.rect, theme.radius, fill);
                    }
                    paint_icon(p, *icon, w.rect, text_color, theme.accent);
                }
                WidgetKind::Swatch { color, selected } => {
                    let ring = if *selected { theme.accent } else { fill };
                    p.fill_rounded_rect(w.rect, theme.radius, ring);
                    let well = inset(w.rect, 5.0);
                    p.fill_rounded_rect(well, 3.0, theme.text);
                    p.fill_rounded_rect(inset(well, 1.0), 2.5, *color);
                }
                WidgetKind::Button { label, dropdown } => {
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
                    p.fill_rounded_rect(inner, 2.0, theme.check_well);
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

fn inset(r: RectF, by: f32) -> RectF {
    RectF { x: r.x + by, y: r.y + by, w: (r.w - 2.0 * by).max(0.0), h: (r.h - 2.0 * by).max(0.0) }
}

/// Draws `icon` in a 12-unit square centered in `rect`.
fn paint_icon(p: &mut Painter, icon: Icon, rect: RectF, color: Color, accent: Color) {
    let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
    match icon {
        Icon::Pause => {
            for x in [cx - 4.5, cx + 1.5] {
                p.fill_rounded_rect(RectF { x, y: cy - 6.0, w: 3.0, h: 12.0 }, 1.0, color);
            }
        }
        Icon::Resume => {
            p.fill_triangle([(cx - 4.0, cy - 6.0), (cx - 4.0, cy + 6.0), (cx + 6.0, cy)], color);
        }
        Icon::Stop => {
            p.fill_rounded_rect(RectF { x: cx - 5.0, y: cy - 5.0, w: 10.0, h: 10.0 }, 2.0, accent);
        }
        Icon::Cancel => {
            p.line((cx - 5.0, cy - 5.0), (cx + 5.0, cy + 5.0), 2.0, color);
            p.line((cx + 5.0, cy - 5.0), (cx - 5.0, cy + 5.0), 2.0, color);
        }
        Icon::Discard => {
            p.line((cx - 6.0, cy - 4.0), (cx + 6.0, cy - 4.0), 2.0, color);
            p.line((cx - 2.0, cy - 6.5), (cx + 2.0, cy - 6.5), 2.0, color);
            p.line((cx - 4.5, cy - 3.0), (cx - 3.5, cy + 6.0), 2.0, color);
            p.line((cx + 4.5, cy - 3.0), (cx + 3.5, cy + 6.0), 2.0, color);
            p.line((cx - 3.5, cy + 6.0), (cx + 3.5, cy + 6.0), 2.0, color);
            p.line((cx, cy - 1.0), (cx, cy + 3.5), 1.5, color);
        }
        Icon::Back | Icon::Undo => {
            p.line((cx + 5.0, cy + 5.0), (cx + 5.0, cy - 1.0), 2.0, color);
            p.line((cx + 5.0, cy - 1.0), (cx - 2.0, cy - 1.0), 2.0, color);
            p.fill_triangle(
                [(cx - 6.5, cy - 1.0), (cx - 2.0, cy - 5.5), (cx - 2.0, cy + 3.5)],
                color,
            );
        }
        Icon::Redo => {
            p.line((cx - 5.0, cy + 5.0), (cx - 5.0, cy - 1.0), 2.0, color);
            p.line((cx - 5.0, cy - 1.0), (cx + 2.0, cy - 1.0), 2.0, color);
            p.fill_triangle(
                [(cx + 6.5, cy - 1.0), (cx + 2.0, cy - 5.5), (cx + 2.0, cy + 3.5)],
                color,
            );
        }
        Icon::Arrow => {
            p.line((cx - 5.5, cy + 5.5), (cx + 2.0, cy - 2.0), 2.0, color);
            p.fill_triangle(
                [(cx + 6.0, cy - 6.0), (cx - 1.0, cy - 4.0), (cx + 4.0, cy + 1.0)],
                color,
            );
        }
        Icon::Rectangle => {
            p.stroke_rect(RectF { x: cx - 6.0, y: cy - 4.5, w: 12.0, h: 9.0 }, 2.0, color);
        }
        Icon::Pen => {
            p.line((cx - 5.0, cy + 5.0), (cx + 4.0, cy - 4.0), 3.0, color);
            p.fill_triangle(
                [(cx - 6.5, cy + 6.5), (cx - 6.0, cy + 3.0), (cx - 3.0, cy + 6.0)],
                accent,
            );
        }
        Icon::Highlighter => {
            p.stroke_polyline(
                &[(cx - 6.0, cy + 3.0), (cx + 6.0, cy + 3.0)],
                5.0,
                Color(0xFDD835),
                220,
            );
            p.line((cx - 3.0, cy + 1.0), (cx + 4.0, cy - 6.0), 3.0, color);
        }
        Icon::Text => {
            p.line((cx - 5.5, cy - 5.0), (cx + 5.5, cy - 5.0), 2.5, color);
            p.line((cx, cy - 5.0), (cx, cy + 6.0), 2.5, color);
        }
        Icon::Pixelate => {
            for (i, (dx, dy)) in
                [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1), (0, 2), (1, 2), (2, 2)]
                    .into_iter()
                    .enumerate()
            {
                let tone = if i % 2 == 0 { color } else { accent };
                let cell = RectF {
                    x: cx - 6.0 + dx as f32 * 4.0,
                    y: cy - 6.0 + dy as f32 * 4.0,
                    w: 4.0,
                    h: 4.0,
                };
                p.fill_rounded_rect(cell, 0.0, tone);
            }
        }
        Icon::Redact => {
            p.fill_rounded_rect(RectF { x: cx - 6.5, y: cy - 4.0, w: 13.0, h: 8.0 }, 1.0, color);
        }
        Icon::Window => {
            p.stroke_rect(RectF { x: cx - 6.5, y: cy - 5.0, w: 13.0, h: 10.0 }, 1.5, color);
            p.fill_rounded_rect(RectF { x: cx - 6.5, y: cy - 5.0, w: 13.0, h: 3.0 }, 0.0, color);
        }
        Icon::FullScreen => {
            let (x0, y0, x1, y1, arm) = (cx - 6.5, cy - 5.0, cx + 6.5, cy + 5.0, 4.0);
            for (x, y, dx, dy) in
                [(x0, y0, 1.0, 1.0), (x1, y0, -1.0, 1.0), (x0, y1, 1.0, -1.0), (x1, y1, -1.0, -1.0)]
            {
                p.line((x, y), (x + dx * arm, y), 1.5, color);
                p.line((x, y), (x, y + dy * arm), 1.5, color);
            }
        }
        Icon::Crop => {
            p.line((cx - 3.0, cy - 7.0), (cx - 3.0, cy + 3.0), 2.0, color);
            p.line((cx - 3.0, cy + 3.0), (cx + 7.0, cy + 3.0), 2.0, color);
            p.line((cx - 7.0, cy - 3.0), (cx + 3.0, cy - 3.0), 2.0, color);
            p.line((cx + 3.0, cy - 3.0), (cx + 3.0, cy + 7.0), 2.0, color);
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
    fn a_click_survives_a_rebuild_between_press_and_release() {
        let f = Fonts::embedded();
        let row =
            || layout_row(vec![(1, btn("Stop")), (2, btn("Cancel"))], 0.0, 0.0, 6.0, &f, &GYOZA);
        let mut old = Ui::new(row());
        let (x, y) = (old.widgets[0].rect.x + 2.0, old.widgets[0].rect.y + 2.0);
        old.pointer_moved(x, y);
        old.pointer_down(x, y);
        let mut new = Ui::new(row());
        new.carry_state_from(&old);
        assert_eq!(new.pointer_up(x, y), Some(1));
    }

    #[test]
    fn tool_button_and_swatch_paint_selection() {
        let f = Fonts::embedded();
        let size =
            natural_size(&WidgetKind::ToolButton { icon: Icon::Pen, selected: true }, &f, &GYOZA);
        assert_eq!(size, (GYOZA.control_height, GYOZA.control_height));
        let edge_color = |kind: WidgetKind| {
            let ws = layout_row(vec![(1, kind)], 0.0, 0.0, 0.0, &f, &GYOZA);
            let mut p = Painter::new(crate::geometry::PixelSize { width: 28, height: 28 }, 1.0);
            p.clear(GYOZA.bg);
            Ui::new(ws).paint(&mut p, &f, &GYOZA);
            let mut out = vec![0; 28 * 28];
            p.to_softbuffer(&mut out);
            out[14 * 28 + 1]
        };
        assert_eq!(
            edge_color(WidgetKind::ToolButton { icon: Icon::Pen, selected: true }),
            GYOZA.accent.0
        );
        assert_ne!(
            edge_color(WidgetKind::ToolButton { icon: Icon::Pen, selected: false }),
            GYOZA.accent.0
        );
        assert_eq!(
            edge_color(WidgetKind::Swatch { color: Color(0x1E88E5), selected: true }),
            GYOZA.accent.0
        );
        assert_ne!(
            edge_color(WidgetKind::Swatch { color: Color(0x1E88E5), selected: false }),
            GYOZA.accent.0
        );
    }

    #[test]
    fn icon_button_is_square() {
        let f = Fonts::embedded();
        let size = natural_size(&WidgetKind::IconButton { icon: Icon::Stop }, &f, &GYOZA);
        assert_eq!(size, (GYOZA.control_height, GYOZA.control_height));
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
