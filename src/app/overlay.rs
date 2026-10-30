//! Full-screen selection overlays, one borderless window per monitor.

use super::surface::WindowSurface;
use gyozarec::capture::{SelectionRequest, WindowCandidate, hovered_window, monitor_at};
use gyozarec::geometry::{CaptureArea, MonitorInfo, PixelPoint, PixelRect, PixelSize, local_point};
use gyozarec::pixels::PixelBuffer;
use gyozarec::presentation::AREAS;
use gyozarec::render::OverlayCache;
use gyozarec::uikit::painter::{Fonts, Painter, RectF, measure_text};
use gyozarec::uikit::theme::{self, Color};
use gyozarec::uikit::widgets::{Icon, Ui, WidgetKind, layout_row};
use std::sync::mpsc::Sender;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{CursorIcon, Window, WindowId, WindowLevel};

const LABEL_PX: f32 = 13.0;
/// Space above the area bar and inside its border, logical units.
const BAR_TOP: f32 = 12.0;
const BAR_PAD: f32 = 6.0;
/// The bar is drawn this many times the size of the other controls, so it
/// is easy to hit on a full screen.
const BAR_ZOOM: f32 = 2.0;
/// Bar ids: the areas in `AREAS` order, then Close.
const ID_AREA: u32 = 1;
const ID_CLOSE: u32 = 10;

fn area_icon(area: CaptureArea) -> Icon {
    match area {
        CaptureArea::Rectangle => Icon::Rectangle,
        CaptureArea::Window => Icon::Window,
        CaptureArea::FullScreen => Icon::FullScreen,
    }
}

/// The buttons that switch the area, then Close, a gap apart.
fn bar_ui(area: CaptureArea, fonts: &Fonts) -> Ui {
    let t = theme::current();
    let tools = AREAS.iter().enumerate().map(|(i, a)| {
        (ID_AREA + i as u32, WidgetKind::ToolButton { icon: area_icon(*a), selected: *a == area })
    });
    let mut widgets = layout_row(tools.collect(), BAR_PAD, BAR_PAD, 4.0, fonts, t);
    let x = widgets.last().map_or(BAR_PAD, |w| w.rect.right()) + 12.0;
    let close = vec![(ID_CLOSE, WidgetKind::IconButton { icon: Icon::Cancel })];
    widgets.extend(layout_row(close, x, BAR_PAD, 0.0, fonts, t));
    Ui::new(widgets)
}

/// The bar at the top of one monitor that switches the area.
struct AreaBar {
    ui: Ui,
    /// Top-left corner on the monitor, local physical pixels.
    origin: PixelPoint,
    size: PixelSize,
    scale: f32,
    /// A press started on the bar, so the release belongs to it.
    pressed: bool,
}

impl AreaBar {
    /// `monitor_scale` is the monitor's; the bar is drawn `BAR_ZOOM` times larger.
    fn new(area: CaptureArea, monitor_width: i32, monitor_scale: f32, fonts: &Fonts) -> AreaBar {
        let ui = bar_ui(area, fonts);
        let b = ui.bounds();
        let scale = monitor_scale * BAR_ZOOM;
        let size = PixelSize {
            width: ((b.right() + BAR_PAD) * scale).ceil() as i32,
            height: ((b.bottom() + BAR_PAD) * scale).ceil() as i32,
        };
        let origin = PixelPoint {
            x: ((monitor_width - size.width) / 2).max(0),
            y: (BAR_TOP * monitor_scale) as i32,
        };
        AreaBar { ui, origin, size, scale, pressed: false }
    }

    fn rebuild(&mut self, area: CaptureArea, fonts: &Fonts) {
        let mut ui = bar_ui(area, fonts);
        ui.carry_state_from(&self.ui);
        self.ui = ui;
    }

    fn contains(&self, p: PixelPoint) -> bool {
        PixelRect::new(self.origin.x, self.origin.y, self.size.width, self.size.height).contains(p)
    }

    /// `p` in the bar's logical units.
    fn local(&self, p: PixelPoint) -> (f32, f32) {
        ((p.x - self.origin.x) as f32 / self.scale, (p.y - self.origin.y) as f32 / self.scale)
    }

    fn bottom(&self) -> i32 {
        self.origin.y + self.size.height
    }

    fn paint(&self, buffer: &mut [u32], width: i32, fonts: &Fonts) {
        let t = theme::current();
        let mut painter = Painter::new(self.size, self.scale);
        painter.clear(t.bg);
        let (w, h) = (self.size.width as f32 / self.scale, self.size.height as f32 / self.scale);
        painter.stroke_rect(RectF { x: 0.5, y: 0.5, w: w - 1.0, h: h - 1.0 }, 1.0, t.disabled_text);
        self.ui.paint(&mut painter, fonts, t);
        painter.blit_into(buffer, width, self.origin);
    }
}

/// What a click on the bar asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BarClick {
    Area(CaptureArea),
    Close,
}

fn bar_click(id: u32) -> Option<BarClick> {
    match id {
        ID_CLOSE => Some(BarClick::Close),
        id => {
            id.checked_sub(ID_AREA).and_then(|i| AREAS.get(i as usize)).map(|a| BarClick::Area(*a))
        }
    }
}

struct OverlayWindow {
    surface: WindowSurface,
    monitor: MonitorInfo,
    cache: OverlayCache,
    buffer: Vec<u32>,
    cursor: PixelPoint,
    drag_start: Option<PixelPoint>,
    /// Shown selection, local pixels.
    selection: Option<PixelRect>,
    shown: bool,
    /// Instruction drawn at the top while picking a monitor.
    hint: Option<String>,
    /// The area bar, on the monitor the selection started on.
    bar: Option<AreaBar>,
    /// The pointer is over the bar (not dragging).
    over_bar: bool,
}

impl OverlayWindow {
    fn local_bounds(&self) -> PixelRect {
        PixelRect::new(0, 0, self.monitor.bounds.width, self.monitor.bounds.height)
    }

    fn to_screen(&self, p: PixelPoint) -> PixelPoint {
        PixelPoint { x: p.x + self.monitor.bounds.x, y: p.y + self.monitor.bounds.y }
    }

    fn redraw(&mut self, area: CaptureArea, fonts: &Fonts) {
        let size = self.cache.size();
        if self.surface.size() != size {
            return; // wait for the resize to land
        }
        self.cache.render(self.selection, &mut self.buffer);
        if let Some(sel) = self.selection.filter(|s| !s.is_empty()) {
            self.draw_label(sel, size, fonts);
        }
        if let Some(hint) = self.hint.clone().filter(|_| area == CaptureArea::FullScreen) {
            self.draw_hint(&hint, size, fonts);
        }
        if let Some(bar) = &self.bar {
            bar.paint(&mut self.buffer, size.width, fonts);
        }
        self.surface.present(&self.buffer);
        if !self.shown {
            // Hidden windows get no WM_PAINT: show now, then paint again.
            self.shown = true;
            self.surface.window.set_visible(true);
            self.surface.window.request_redraw();
        }
    }

    /// A short instruction centred at the top of the monitor.
    fn draw_hint(&mut self, text: &str, size: PixelSize, fonts: &Fonts) {
        let scale = self.monitor.scale as f32;
        let (tw, th) = measure_text(fonts, text, LABEL_PX, false);
        let (pad_x, pad_y) = (10.0, 6.0);
        let label = PixelSize {
            width: ((tw + 2.0 * pad_x) * scale).ceil() as i32,
            height: ((th + 2.0 * pad_y) * scale).ceil() as i32,
        };
        let mut painter = Painter::new(label, scale);
        painter.clear(Color(0x202020));
        painter.text(fonts, text, pad_x, pad_y, LABEL_PX, Color(0xFFFFFF), false);
        let x = ((size.width - label.width) / 2).max(0);
        let y = match &self.bar {
            Some(bar) => bar.bottom() + (8.0 * scale) as i32,
            None => (24.0 * scale) as i32,
        };
        painter.blit_into(&mut self.buffer, size.width, PixelPoint { x, y });
    }

    fn draw_label(&mut self, sel: PixelRect, size: PixelSize, fonts: &Fonts) {
        let scale = self.monitor.scale as f32;
        let text = format!("{} × {}", sel.width, sel.height);
        let (tw, th) = measure_text(fonts, &text, LABEL_PX, false);
        let (pad_x, pad_y) = (6.0, 3.0);
        let label = PixelSize {
            width: ((tw + 2.0 * pad_x) * scale).ceil() as i32,
            height: ((th + 2.0 * pad_y) * scale).ceil() as i32,
        };
        let mut painter = Painter::new(label, scale);
        painter.clear(Color(0x202020));
        painter.text(fonts, &text, pad_x, pad_y, LABEL_PX, Color(0xFFFFFF), false);
        let gap = (6.0 * scale) as i32;
        let mut y = sel.bottom() + gap;
        if y + label.height > size.height {
            y = (sel.bottom() - label.height - gap).max(0);
        }
        let x = (sel.right() - label.width).clamp(0, (size.width - label.width).max(0));
        painter.blit_into(&mut self.buffer, size.width, PixelPoint { x, y });
    }
}

/// What a hover highlights: the window under the point (Window mode) or the
/// whole monitor (full-screen monitor pick); nothing while dragging a rectangle.
fn initial_hover(
    area: CaptureArea,
    candidates: &[WindowCandidate],
    monitors: &[PixelRect],
    at: PixelPoint,
) -> Option<PixelRect> {
    match area {
        CaptureArea::Window => hovered_window(candidates, monitors, at),
        CaptureArea::FullScreen => monitor_at(monitors, at),
        CaptureArea::Rectangle => None,
    }
}

pub struct OverlaySession {
    windows: Vec<OverlayWindow>,
    area: CaptureArea,
    candidates: Vec<WindowCandidate>,
    reply: Option<Sender<Option<PixelRect>>>,
    /// Window-mode pick under the cursor, screen pixels.
    hovered: Option<PixelRect>,
    /// The area bar changed the area.
    switched: bool,
}

impl OverlaySession {
    pub fn open(
        event_loop: &ActiveEventLoop,
        request: SelectionRequest,
        reply: Sender<Option<PixelRect>>,
        cursor: PixelPoint,
        hint: Option<&str>,
        fonts: &Fonts,
    ) -> OverlaySession {
        let empty = PixelBuffer::solid(PixelRect::default(), 0, 0, 0);
        let frozen = request.frozen.clone();
        let mut windows = Vec::new();
        for monitor in &request.monitors {
            let b = monitor.bounds;
            let attrs = Window::default_attributes()
                .with_title("GyozaRec")
                .with_decorations(false)
                .with_resizable(false)
                .with_visible(false)
                .with_window_level(WindowLevel::AlwaysOnTop)
                .with_skip_taskbar(true)
                .with_position(PhysicalPosition::new(b.x, b.y))
                .with_inner_size(PhysicalSize::new(b.width as u32, b.height as u32));
            let Ok(window) = event_loop.create_window(attrs) else { continue };
            window.set_cursor(CursorIcon::Crosshair);
            let part = frozen
                .as_ref()
                .map(|f| f.crop(&b).with_origin(PixelPoint::default()))
                .unwrap_or_else(|| empty.clone());
            let part = if part.bounds.size() == b.size() {
                part
            } else {
                PixelBuffer::solid(PixelRect::new(0, 0, b.width, b.height), 0, 0, 0)
            };
            let cache = OverlayCache::new(&part);
            windows.push(OverlayWindow {
                surface: WindowSurface::pinned(window, b),
                monitor: monitor.clone(),
                buffer: vec![0; (b.width * b.height) as usize],
                cache,
                cursor: PixelPoint::default(),
                drag_start: None,
                selection: None,
                shown: false,
                hint: hint.map(str::to_string),
                bar: None,
                over_bar: false,
            });
        }
        if request.switcher {
            let home = windows.iter().position(|w| w.monitor.bounds.contains(cursor)).unwrap_or(0);
            if let Some(w) = windows.get_mut(home) {
                let scale = w.monitor.scale as f32;
                w.bar = Some(AreaBar::new(request.area, w.monitor.bounds.width, scale, fonts));
            }
        }
        let mut session =
            OverlaySession::new(windows, request.area, request.windows, reply, cursor);
        let area = session.area;
        for w in &mut session.windows {
            w.redraw(area, fonts);
            if w.monitor.bounds.contains(cursor) {
                w.surface.window.focus_window();
            }
        }
        session
    }

    /// Starts from the real cursor, so a click before any mouse movement
    /// works; with no window to select on, the selection ends at once.
    fn new(
        mut windows: Vec<OverlayWindow>,
        area: CaptureArea,
        candidates: Vec<WindowCandidate>,
        reply: Sender<Option<PixelRect>>,
        cursor: PixelPoint,
    ) -> OverlaySession {
        for w in &mut windows {
            w.cursor = local_point(cursor, w.monitor.bounds);
        }
        let mut session = OverlaySession {
            windows,
            area,
            candidates,
            reply: Some(reply),
            hovered: None,
            switched: false,
        };
        if session.windows.is_empty() {
            session.finish(None);
        } else if area != CaptureArea::Rectangle {
            session.update_hover(cursor);
        }
        session
    }

    /// Window mode: highlights the window under `screen` on every monitor.
    fn update_hover(&mut self, screen: PixelPoint) {
        let monitors: Vec<PixelRect> = self.windows.iter().map(|o| o.monitor.bounds).collect();
        let hovered = initial_hover(self.area, &self.candidates, &monitors, screen);
        if hovered == self.hovered {
            return;
        }
        self.hovered = hovered;
        for other in &mut self.windows {
            let local = hovered
                .map(|r| r.offset(-other.monitor.bounds.x, -other.monitor.bounds.y))
                .map(|r| r.intersect(&other.local_bounds()))
                .filter(|r| !r.is_empty());
            if local != other.selection {
                other.selection = local;
                other.surface.window.request_redraw();
            }
        }
    }

    pub fn owns(&self, id: WindowId) -> bool {
        self.windows.iter().any(|w| w.surface.window.id() == id)
    }

    pub fn surface(&self, id: WindowId) -> Option<&WindowSurface> {
        self.windows.iter().map(|w| &w.surface).find(|s| s.window.id() == id)
    }

    fn finish(&mut self, result: Option<PixelRect>) {
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(result);
        }
    }

    pub fn is_finished(&self) -> bool {
        self.reply.is_none()
    }

    /// Cancels if the session is dropped without an answer.
    pub fn cancel(&mut self) {
        self.finish(None);
    }

    pub fn handle(&mut self, id: WindowId, event: &WindowEvent, fonts: &Fonts) {
        let Some(index) = self.windows.iter().position(|w| w.surface.window.id() == id) else {
            return;
        };
        match event {
            WindowEvent::RedrawRequested => self.windows[index].redraw(self.area, fonts),
            // Geometry after DPI changes is restored by WindowSurface::keep_on_target.
            WindowEvent::ScaleFactorChanged { .. } | WindowEvent::Resized(_) => {
                self.windows[index].surface.window.request_redraw()
            }
            WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key: Key::Named(NamedKey::Escape), state, .. },
                ..
            } if *state == ElementState::Pressed => self.finish(None),
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Right,
                ..
            } => self.finish(None),
            WindowEvent::CursorMoved { position, .. } => {
                let p = PixelPoint { x: position.x as i32, y: position.y as i32 };
                self.pointer_moved(index, p);
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                let pressed = *state == ElementState::Pressed;
                if !self.bar_button(index, pressed, fonts) {
                    self.left_button(index, pressed);
                }
            }
            _ => {}
        }
    }

    /// The area chosen on the bar, when it differs from the one asked for.
    pub fn switched_area(&self) -> Option<CaptureArea> {
        self.switched.then_some(self.area)
    }

    /// Hover over the bar; `true` while the pointer is on it.
    fn bar_hover(&mut self, index: usize, p: PixelPoint) -> bool {
        let w = &mut self.windows[index];
        let dragging = w.drag_start.is_some();
        let Some(bar) = w.bar.as_mut() else { return false };
        let over = !dragging && bar.contains(p);
        let changed = if over {
            let (x, y) = bar.local(p);
            bar.ui.pointer_moved(x, y)
        } else {
            bar.ui.pointer_left()
        };
        if over != w.over_bar {
            w.over_bar = over;
            w.surface.window.set_cursor(if over {
                CursorIcon::Default
            } else {
                CursorIcon::Crosshair
            });
        }
        if changed {
            w.surface.window.request_redraw();
        }
        over
    }

    /// A press or release that belongs to the bar; `false` leaves it to the selection.
    fn bar_button(&mut self, index: usize, pressed: bool, fonts: &Fonts) -> bool {
        let w = &mut self.windows[index];
        let cursor = w.cursor;
        let over = w.over_bar;
        let Some(bar) = w.bar.as_mut() else { return false };
        let (x, y) = bar.local(cursor);
        if pressed {
            if !over {
                return false;
            }
            bar.ui.pointer_down(x, y);
            bar.pressed = true;
            w.surface.window.request_redraw();
            return true;
        }
        if !std::mem::take(&mut bar.pressed) {
            return false;
        }
        let clicked = bar.ui.pointer_up(x, y).and_then(bar_click);
        w.surface.window.request_redraw();
        match clicked {
            Some(BarClick::Close) => self.finish(None),
            Some(BarClick::Area(area)) => self.switch_area(index, area, fonts),
            None => {}
        }
        true
    }

    /// Starts over with another area; full screen on a single monitor
    /// captures it at once.
    fn switch_area(&mut self, index: usize, area: CaptureArea, fonts: &Fonts) {
        self.switched |= area != self.area;
        self.area = area;
        if area == CaptureArea::FullScreen && self.windows.len() == 1 {
            let bounds = self.windows[0].monitor.bounds;
            self.finish(Some(bounds));
            return;
        }
        self.hovered = None;
        for w in &mut self.windows {
            w.drag_start = None;
            w.selection = None;
            if let Some(bar) = w.bar.as_mut() {
                bar.rebuild(area, fonts);
            }
            w.surface.window.request_redraw();
        }
        if area != CaptureArea::Rectangle {
            let screen = self.windows[index].to_screen(self.windows[index].cursor);
            self.update_hover(screen);
        }
    }

    fn pointer_moved(&mut self, index: usize, p: PixelPoint) {
        self.windows[index].cursor = p;
        if self.bar_hover(index, p) {
            return;
        }
        let w = &mut self.windows[index];
        if self.area != CaptureArea::Rectangle {
            let screen = w.to_screen(p);
            self.update_hover(screen);
        } else if let Some(start) = w.drag_start {
            let sel = PixelRect::from_points(start, p).clamp_to(&w.local_bounds());
            if Some(sel) != w.selection {
                w.selection = Some(sel);
                w.surface.window.request_redraw();
            }
        }
    }

    fn left_button(&mut self, index: usize, pressed: bool) {
        if self.area != CaptureArea::Rectangle {
            if !pressed {
                let pick = self.hovered.unwrap_or_default();
                self.finish(Some(pick));
            }
            return;
        }
        let w = &mut self.windows[index];
        if pressed {
            w.drag_start = Some(w.cursor);
            w.selection = Some(PixelRect::from_points(w.cursor, w.cursor));
        } else if w.drag_start.take().is_some() {
            let local = w.selection.unwrap_or_default();
            let result =
                local.offset(w.monitor.bounds.x, w.monitor.bounds.y).clamp_to(&w.monitor.bounds);
            self.finish(Some(result));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_buttons_map_to_areas_and_close() {
        assert_eq!(bar_click(ID_AREA), Some(BarClick::Area(CaptureArea::Rectangle)));
        assert_eq!(bar_click(ID_AREA + 1), Some(BarClick::Area(CaptureArea::Window)));
        assert_eq!(bar_click(ID_AREA + 2), Some(BarClick::Area(CaptureArea::FullScreen)));
        assert_eq!(bar_click(ID_AREA + 3), None);
        assert_eq!(bar_click(ID_CLOSE), Some(BarClick::Close));
    }

    #[test]
    fn bar_sits_centred_at_the_top_and_marks_the_area() {
        let fonts = Fonts::embedded();
        let bar = AreaBar::new(CaptureArea::Window, 1920, 1.5, &fonts);
        assert_eq!(bar.origin.y, 18, "the gap above follows the monitor, not the zoom");
        assert!((bar.origin.x * 2 + bar.size.width - 1920).abs() <= 1, "{:?}", bar.origin);
        let b = bar.ui.bounds();
        let plain = ((b.bottom() + BAR_PAD) * 1.5).ceil() as i32;
        assert!(bar.size.height >= 2 * plain - 1, "twice as tall: {}", bar.size.height);
        let s = bar.scale;
        let centre = |id: u32| {
            let r = bar.ui.widget(id).unwrap().rect;
            let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
            PixelPoint { x: bar.origin.x + (x * s) as i32, y: bar.origin.y + (y * s) as i32 }
        };
        let close = centre(ID_CLOSE);
        assert!(bar.contains(close));
        assert_eq!(bar.ui.hit_test(bar.local(close).0, bar.local(close).1), Some(ID_CLOSE));
        let selected: Vec<bool> = (0..3)
            .map(|i| match bar.ui.widget(ID_AREA + i).unwrap().kind {
                WidgetKind::ToolButton { selected, .. } => selected,
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(selected, [false, true, false]);
    }

    #[test]
    fn initial_hover_follows_the_cursor() {
        let monitors = [PixelRect::new(-1920, 0, 1920, 1080), PixelRect::new(0, 0, 1920, 1080)];
        let at = PixelPoint { x: -5, y: 5 };
        assert_eq!(initial_hover(CaptureArea::FullScreen, &[], &monitors, at), Some(monitors[0]));
        assert_eq!(initial_hover(CaptureArea::Rectangle, &[], &monitors, at), None);
    }

    #[test]
    fn monitor_pick_without_windows_replies_none() {
        let (reply, answer) = std::sync::mpsc::channel();
        let session = OverlaySession::new(
            Vec::new(),
            CaptureArea::FullScreen,
            Vec::new(),
            reply,
            PixelPoint::default(),
        );
        assert!(session.is_finished());
        assert_eq!(answer.try_recv(), Ok(None));
    }

    #[test]
    fn overlay_session_without_windows_replies_none() {
        let (reply, answer) = std::sync::mpsc::channel();
        let session = OverlaySession::new(
            Vec::new(),
            CaptureArea::Rectangle,
            Vec::new(),
            reply,
            PixelPoint::default(),
        );
        assert!(session.is_finished());
        assert_eq!(answer.try_recv(), Ok(None));
    }
}
