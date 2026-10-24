//! Red frame around the recorded area and the Stop/Cancel panel.

use super::surface::WindowSurface;
use gyozarec::bridge::RecordingCommand;
use gyozarec::geometry::{MonitorInfo, PixelRect, PixelSize};
use gyozarec::platform::{exclude_from_capture, make_color_keyed};
use gyozarec::presentation::format_elapsed;
use gyozarec::recording::{frame_bounds, place_panel, render_frame};
use gyozarec::uikit::painter::{Fonts, Painter};
use gyozarec::uikit::theme::GYOZA;
use gyozarec::uikit::widgets::{Ui, WidgetKind, layout_row};
use std::sync::mpsc::Sender;
use std::time::Duration;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{Window, WindowId, WindowLevel};

const FRAME_THICKNESS: i32 = 2;
/// Color made transparent (and click-through) inside the frame window.
const FRAME_KEY: u32 = 0x00FF00FF;
const PAD: f32 = 8.0;
const DOT: f32 = 14.0;
const ID_TIME: u32 = 1;
const ID_STOP: u32 = 2;
const ID_CANCEL: u32 = 3;

fn popup(event_loop: &ActiveEventLoop, rect: PixelRect) -> Option<WindowSurface> {
    let attrs = Window::default_attributes()
        .with_title("GyozaRec")
        .with_decorations(false)
        .with_resizable(false)
        .with_visible(false)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_skip_taskbar(true)
        .with_position(PhysicalPosition::new(rect.x, rect.y))
        .with_inner_size(PhysicalSize::new(rect.width.max(1) as u32, rect.height.max(1) as u32));
    let surface = WindowSurface::new(event_loop.create_window(attrs).ok()?);
    exclude_from_capture(surface.hwnd());
    Some(surface)
}

fn build_panel_ui(elapsed: Duration, max: Duration, fonts: &Fonts) -> Ui {
    let button = |label: &str| WidgetKind::Button { label: label.into(), dropdown: false };
    Ui::new(layout_row(
        vec![
            (ID_TIME, WidgetKind::Label { text: format_elapsed(elapsed, max), color: GYOZA.text }),
            (ID_STOP, button("Stop")),
            (ID_CANCEL, button("Annulla")),
        ],
        PAD + DOT + 6.0,
        PAD,
        10.0,
        fonts,
        &GYOZA,
    ))
}

pub struct RecordingSession {
    frame: Option<WindowSurface>,
    panel: WindowSurface,
    ui: Ui,
    scale: f32,
    cursor: (f32, f32),
    max: Duration,
    commands: Sender<RecordingCommand>,
}

impl RecordingSession {
    pub fn open(
        event_loop: &ActiveEventLoop,
        region: PixelRect,
        monitor: &MonitorInfo,
        max: Duration,
        commands: Sender<RecordingCommand>,
        fonts: &Fonts,
    ) -> Option<RecordingSession> {
        let mut frame = popup(event_loop, frame_bounds(region, FRAME_THICKNESS));
        if let Some(f) = &mut frame {
            make_color_keyed(f.hwnd(), FRAME_KEY);
            paint_frame(f);
            f.window.set_visible(true);
        }
        let scale = monitor.scale as f32;
        let ui = build_panel_ui(Duration::ZERO, max, fonts);
        let b = ui.bounds();
        let size = PixelSize {
            width: ((b.right() + PAD) * scale).ceil() as i32,
            height: ((b.bottom() + PAD) * scale).ceil() as i32,
        };
        let at = place_panel(region, size, monitor.bounds);
        let panel = popup(event_loop, PixelRect::new(at.x, at.y, size.width, size.height))?;
        let mut session =
            RecordingSession { frame, panel, ui, scale, cursor: (-1.0, -1.0), max, commands };
        session.redraw(fonts);
        session.panel.window.set_visible(true);
        session.panel.window.focus_window();
        Some(session)
    }

    pub fn owns(&self, id: WindowId) -> bool {
        self.panel.window.id() == id || self.frame.as_ref().is_some_and(|f| f.window.id() == id)
    }

    pub fn tick(&mut self, elapsed: Duration, fonts: &Fonts) {
        self.ui = build_panel_ui(elapsed, self.max, fonts);
        self.panel.window.request_redraw();
    }

    fn redraw(&mut self, fonts: &Fonts) {
        let size = self.panel.size();
        if size.width <= 0 || size.height <= 0 {
            return;
        }
        let mut painter = Painter::new(size, self.scale);
        painter.clear(GYOZA.bg);
        let time = self.ui.widget(ID_TIME).map(|w| w.rect).unwrap_or_default();
        painter.fill_circle(PAD + DOT / 2.0, time.y + time.h / 2.0, DOT / 2.0 - 1.0, GYOZA.accent);
        self.ui.paint(&mut painter, fonts, &GYOZA);
        let mut pixels = vec![0u32; (size.width * size.height) as usize];
        painter.to_softbuffer(&mut pixels);
        self.panel.present(&pixels);
    }

    pub fn handle(&mut self, id: WindowId, event: &WindowEvent, fonts: &Fonts) {
        if let Some(frame) = self.frame.as_mut().filter(|f| f.window.id() == id) {
            if let WindowEvent::RedrawRequested = event {
                paint_frame(frame);
            }
            return;
        }
        match event {
            WindowEvent::RedrawRequested => self.redraw(fonts),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as f32 / self.scale, position.y as f32 / self.scale);
                if self.ui.pointer_moved(self.cursor.0, self.cursor.1) {
                    self.panel.window.request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                self.panel.window.request_redraw();
                if *state == ElementState::Pressed {
                    self.ui.pointer_down(self.cursor.0, self.cursor.1);
                } else {
                    match self.ui.pointer_up(self.cursor.0, self.cursor.1) {
                        Some(ID_STOP) => {
                            let _ = self.commands.send(RecordingCommand::Stop);
                        }
                        Some(ID_CANCEL) => {
                            let _ = self.commands.send(RecordingCommand::Cancel);
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key: Key::Named(NamedKey::Escape), state, .. },
                ..
            } if *state == ElementState::Pressed => {
                let _ = self.commands.send(RecordingCommand::Cancel);
            }
            _ => {}
        }
    }
}

fn paint_frame(frame: &mut WindowSurface) {
    let size = frame.size();
    let mut pixels = vec![0u32; (size.width.max(0) * size.height.max(0)) as usize];
    render_frame(size, FRAME_THICKNESS, GYOZA.accent.0, FRAME_KEY, &mut pixels);
    frame.present(&pixels);
}
