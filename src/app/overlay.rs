//! Full-screen selection overlays, one borderless window per monitor.

use super::surface::WindowSurface;
use gyozarec::capture::{SelectionRequest, WindowCandidate, hovered_window};
use gyozarec::geometry::{CaptureArea, MonitorInfo, PixelPoint, PixelRect, PixelSize, local_point};
use gyozarec::pixels::PixelBuffer;
use gyozarec::render::OverlayCache;
use gyozarec::uikit::painter::{Fonts, Painter, measure_text};
use gyozarec::uikit::theme::Color;
use std::sync::mpsc::Sender;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{CursorIcon, Window, WindowId, WindowLevel};

const LABEL_PX: f32 = 13.0;

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
}

impl OverlayWindow {
    fn local_bounds(&self) -> PixelRect {
        PixelRect::new(0, 0, self.monitor.bounds.width, self.monitor.bounds.height)
    }

    fn to_screen(&self, p: PixelPoint) -> PixelPoint {
        PixelPoint { x: p.x + self.monitor.bounds.x, y: p.y + self.monitor.bounds.y }
    }

    fn redraw(&mut self, fonts: &Fonts) {
        let size = self.cache.size();
        if self.surface.size() != size {
            return; // wait for the resize to land
        }
        self.cache.render(self.selection, &mut self.buffer);
        if let Some(sel) = self.selection.filter(|s| !s.is_empty()) {
            self.draw_label(sel, size, fonts);
        }
        self.surface.present(&self.buffer);
        if !self.shown {
            // Hidden windows get no WM_PAINT: show now, then paint again.
            self.shown = true;
            self.surface.window.set_visible(true);
            self.surface.window.request_redraw();
        }
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

pub struct OverlaySession {
    windows: Vec<OverlayWindow>,
    area: CaptureArea,
    candidates: Vec<WindowCandidate>,
    reply: Option<Sender<Option<PixelRect>>>,
    /// Window-mode pick under the cursor, screen pixels.
    hovered: Option<PixelRect>,
}

impl OverlaySession {
    pub fn open(
        event_loop: &ActiveEventLoop,
        request: SelectionRequest,
        reply: Sender<Option<PixelRect>>,
        cursor: PixelPoint,
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
            });
        }
        let mut session =
            OverlaySession::new(windows, request.area, request.windows, reply, cursor);
        for w in &mut session.windows {
            w.redraw(fonts);
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
        let mut session =
            OverlaySession { windows, area, candidates, reply: Some(reply), hovered: None };
        if session.windows.is_empty() {
            session.finish(None);
        } else if area == CaptureArea::Window {
            session.update_hover(cursor);
        }
        session
    }

    /// Window mode: highlights the window under `screen` on every monitor.
    fn update_hover(&mut self, screen: PixelPoint) {
        let monitors: Vec<PixelRect> = self.windows.iter().map(|o| o.monitor.bounds).collect();
        let hovered = hovered_window(&self.candidates, &monitors, screen);
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
            WindowEvent::RedrawRequested => self.windows[index].redraw(fonts),
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
                self.left_button(index, *state == ElementState::Pressed);
            }
            _ => {}
        }
    }

    fn pointer_moved(&mut self, index: usize, p: PixelPoint) {
        let w = &mut self.windows[index];
        w.cursor = p;
        if self.area == CaptureArea::Window {
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
        if self.area == CaptureArea::Window {
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
