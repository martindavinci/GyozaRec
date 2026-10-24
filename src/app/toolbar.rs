//! The main toolbar window: four buttons and a status line.

use super::surface::WindowSurface;
use gyozarec::geometry::PixelSize;
use gyozarec::presentation::{ToolbarState, mode_label};
use gyozarec::uikit::painter::{Fonts, Painter};
use gyozarec::uikit::theme::GYOZA;
use gyozarec::uikit::widgets::{Ui, WidgetKind, layout_row};
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{Window, WindowButtons};

const PAD: f32 = 8.0;
/// Keeps the title readable when the labels are short (e.g. Chinese).
const MIN_WIDTH: f32 = 320.0;
const ID_CAPTURE: u32 = 1;
const ID_MODE: u32 = 2;
const ID_DELAY: u32 = 3;
const ID_OPTIONS: u32 = 4;
const ID_STATUS: u32 = 5;
const ID_FOLDER: u32 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarAction {
    Capture,
    ModeMenu,
    DelayMenu,
    Options,
    OpenFolder,
}

pub struct Toolbar {
    pub surface: WindowSurface,
    ui: Ui,
    logical: (f32, f32),
    cursor: (f32, f32),
}

fn build_ui(state: &ToolbarState, fonts: &Fonts) -> Ui {
    let texts = state.texts();
    let button = |label: &str, dropdown: bool| WidgetKind::Button { label: label.into(), dropdown };
    let mut widgets = layout_row(
        vec![
            (ID_CAPTURE, button(texts.capture, false)),
            (ID_MODE, button(mode_label(state.settings.mode, texts), true)),
            (ID_DELAY, button(texts.delay, true)),
            (ID_OPTIONS, button(texts.options, false)),
        ],
        PAD,
        PAD,
        6.0,
        fonts,
        &GYOZA,
    );
    for w in &mut widgets {
        w.enabled = !state.busy;
    }
    let row1_bottom = widgets.iter().map(|w| w.rect.bottom()).fold(0.0, f32::max);
    let color = if state.status_is_error { GYOZA.error } else { GYOZA.text };
    let mut row2 = vec![(ID_STATUS, WidgetKind::Label { text: state.status.clone(), color })];
    if state.last_saved.is_some() {
        row2.push((ID_FOLDER, WidgetKind::Link { text: texts.open_folder.into() }));
    }
    widgets.extend(layout_row(row2, PAD, row1_bottom + 8.0, 12.0, fonts, &GYOZA));
    Ui::new(widgets)
}

fn logical_size(ui: &Ui) -> (f32, f32) {
    let b = ui.bounds();
    ((b.right() + PAD).ceil().max(MIN_WIDTH), (b.bottom() + PAD).ceil())
}

impl Toolbar {
    pub fn create(event_loop: &ActiveEventLoop, state: &ToolbarState, fonts: &Fonts) -> Toolbar {
        let ui = build_ui(state, fonts);
        let logical = logical_size(&ui);
        let attrs = Window::default_attributes()
            .with_title("GyozaRec")
            .with_window_icon(super::window_icon())
            .with_taskbar_icon(super::window_icon())
            .with_resizable(false)
            .with_enabled_buttons(WindowButtons::CLOSE | WindowButtons::MINIMIZE)
            .with_inner_size(LogicalSize::new(logical.0, logical.1));
        let window = event_loop.create_window(attrs).expect("toolbar window");
        Toolbar { surface: WindowSurface::new(window), ui, logical, cursor: (-1.0, -1.0) }
    }

    fn scale(&self) -> f32 {
        self.surface.window.scale_factor() as f32
    }

    /// Rebuilds widgets from `state` and resizes the window to fit them.
    pub fn rebuild(&mut self, state: &ToolbarState, fonts: &Fonts) {
        self.ui = build_ui(state, fonts);
        let logical = logical_size(&self.ui);
        if logical != self.logical {
            self.logical = logical;
            let _ = self.surface.window.request_inner_size(LogicalSize::new(logical.0, logical.1));
        }
        self.surface.window.request_redraw();
    }

    pub fn redraw(&mut self, fonts: &Fonts) {
        let size = self.surface.size();
        if size.width <= 0 || size.height <= 0 {
            return;
        }
        let mut painter = Painter::new(size, self.scale());
        painter.clear(GYOZA.bg);
        self.ui.paint(&mut painter, fonts, &GYOZA);
        let mut pixels = vec![0u32; (size.width * size.height) as usize];
        painter.to_softbuffer(&mut pixels);
        self.surface.present(&pixels);
    }

    /// Bottom-left corner of a dropdown button, in client physical pixels.
    pub fn menu_anchor(&self, action: ToolbarAction) -> PhysicalPosition<i32> {
        let id = if action == ToolbarAction::DelayMenu { ID_DELAY } else { ID_MODE };
        let scale = self.scale();
        let rect = self.ui.widget(id).map(|w| w.rect).unwrap_or_default();
        PhysicalPosition::new((rect.x * scale) as i32, (rect.bottom() * scale) as i32)
    }

    pub fn handle(&mut self, event: &WindowEvent, fonts: &Fonts) -> Option<ToolbarAction> {
        match event {
            WindowEvent::RedrawRequested => self.redraw(fonts),
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.scale();
                self.cursor = (position.x as f32 / scale, position.y as f32 / scale);
                if self.ui.pointer_moved(self.cursor.0, self.cursor.1) {
                    self.surface.window.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if self.ui.pointer_left() {
                    self.surface.window.request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                self.surface.window.request_redraw();
                if *state == ElementState::Pressed {
                    self.ui.pointer_down(self.cursor.0, self.cursor.1);
                    return None;
                }
                return match self.ui.pointer_up(self.cursor.0, self.cursor.1)? {
                    ID_CAPTURE => Some(ToolbarAction::Capture),
                    ID_MODE => Some(ToolbarAction::ModeMenu),
                    ID_DELAY => Some(ToolbarAction::DelayMenu),
                    ID_OPTIONS => Some(ToolbarAction::Options),
                    ID_FOLDER => Some(ToolbarAction::OpenFolder),
                    _ => None,
                };
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let s = *scale_factor as f32;
                let size = PixelSize {
                    width: (self.logical.0 * s).ceil() as i32,
                    height: (self.logical.1 * s).ceil() as i32,
                };
                let _ = self.surface.window.request_inner_size(winit::dpi::PhysicalSize::new(
                    size.width as u32,
                    size.height as u32,
                ));
                self.surface.window.request_redraw();
            }
            _ => {}
        }
        None
    }
}
