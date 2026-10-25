//! Red frame around the recorded area and the Pause/Stop/Cancel panel.

use super::surface::WindowSurface;
use gyozarec::bridge::RecordingCommand;
use gyozarec::geometry::{MonitorInfo, PixelRect, PixelSize};
use gyozarec::i18n::Texts;
use gyozarec::platform::{exclude_from_capture, make_color_keyed};
use gyozarec::presentation::format_elapsed;
use gyozarec::recording::{
    PanelAction, PanelInput, PanelMode, frame_bounds, panel_step, place_panel, render_frame,
};
use gyozarec::uikit::painter::{Fonts, Painter};
use gyozarec::uikit::theme::GYOZA;
use gyozarec::uikit::widgets::{Icon, Ui, WidgetKind, layout_row};
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
const ID_PAUSE: u32 = 4;
const ID_QUESTION: u32 = 5;
const ID_CONFIRM: u32 = 6;
const ID_BACK: u32 = 7;

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
    let surface = WindowSurface::pinned(event_loop.create_window(attrs).ok()?, rect);
    exclude_from_capture(surface.hwnd());
    Some(surface)
}

fn build_panel_ui(
    elapsed: Duration,
    max: Duration,
    paused: bool,
    mode: PanelMode,
    texts: &Texts,
    fonts: &Fonts,
) -> Ui {
    let button = |icon| WidgetKind::IconButton { icon };
    let items = match mode {
        PanelMode::Normal => vec![
            (ID_TIME, WidgetKind::Label { text: format_elapsed(elapsed, max), color: GYOZA.text }),
            (ID_PAUSE, button(if paused { Icon::Resume } else { Icon::Pause })),
            (ID_STOP, button(Icon::Stop)),
            (ID_CANCEL, button(Icon::Discard)),
        ],
        PanelMode::Confirming => vec![
            (
                ID_QUESTION,
                WidgetKind::Label { text: texts.discard_question.into(), color: GYOZA.text },
            ),
            (ID_CONFIRM, button(Icon::Discard)),
            (ID_BACK, button(Icon::Back)),
        ],
    };
    // The question has no recording dot in front of it.
    let x = if mode == PanelMode::Normal { PAD + DOT + 6.0 } else { PAD };
    Ui::new(layout_row(items, x, PAD, 10.0, fonts, &GYOZA))
}

/// Logical size that fits every layout the panel can show, so switching to
/// the question never clips it.
fn panel_logical_size(max: Duration, texts: &Texts, fonts: &Fonts) -> (f32, f32) {
    let layouts =
        [(false, PanelMode::Normal), (true, PanelMode::Normal), (false, PanelMode::Confirming)];
    layouts.iter().fold((0.0, 0.0), |(w, h), &(paused, mode)| {
        let b = build_panel_ui(max, max, paused, mode, texts, fonts).bounds();
        (f32::max(w, b.right() + PAD), f32::max(h, b.bottom() + PAD))
    })
}

fn paint_panel(painter: &mut Painter, ui: &Ui, paused: bool, fonts: &Fonts) {
    painter.clear(GYOZA.bg);
    if let Some(time) = ui.widget(ID_TIME).map(|w| w.rect) {
        let dot = if paused { GYOZA.disabled_text } else { GYOZA.accent };
        painter.fill_circle(PAD + DOT / 2.0, time.y + time.h / 2.0, DOT / 2.0 - 1.0, dot);
    }
    ui.paint(painter, fonts, &GYOZA);
}

pub struct RecordingSession {
    frame: Option<WindowSurface>,
    panel: WindowSurface,
    ui: Ui,
    scale: f32,
    cursor: (f32, f32),
    max: Duration,
    elapsed: Duration,
    paused: bool,
    mode: PanelMode,
    texts: &'static Texts,
    commands: Sender<RecordingCommand>,
}

impl RecordingSession {
    pub fn open(
        event_loop: &ActiveEventLoop,
        region: PixelRect,
        monitor: &MonitorInfo,
        max: Duration,
        commands: Sender<RecordingCommand>,
        texts: &'static Texts,
        fonts: &Fonts,
    ) -> Option<RecordingSession> {
        let frame = open_frame(event_loop, region);
        let scale = monitor.scale as f32;
        let ui = build_panel_ui(Duration::ZERO, max, false, PanelMode::Normal, texts, fonts);
        let (w, h) = panel_logical_size(max, texts, fonts);
        let size =
            PixelSize { width: (w * scale).ceil() as i32, height: (h * scale).ceil() as i32 };
        let at = place_panel(region, size, monitor.work_area);
        let panel = popup(event_loop, PixelRect::new(at.x, at.y, size.width, size.height))?;
        let mut session = RecordingSession {
            frame,
            panel,
            ui,
            scale,
            cursor: (-1.0, -1.0),
            max,
            elapsed: Duration::ZERO,
            paused: false,
            mode: PanelMode::Normal,
            texts,
            commands,
        };
        session.redraw(fonts);
        session.panel.window.set_visible(true);
        session.panel.window.focus_window();
        Some(session)
    }

    pub fn owns(&self, id: WindowId) -> bool {
        self.surface(id).is_some()
    }

    pub fn surface(&self, id: WindowId) -> Option<&WindowSurface> {
        std::iter::once(&self.panel).chain(self.frame.as_ref()).find(|s| s.window.id() == id)
    }

    pub fn tick(&mut self, elapsed: Duration, fonts: &Fonts) {
        self.elapsed = elapsed;
        self.rebuild(fonts);
    }

    fn rebuild(&mut self, fonts: &Fonts) {
        let mut ui =
            build_panel_ui(self.elapsed, self.max, self.paused, self.mode, self.texts, fonts);
        ui.carry_state_from(&self.ui);
        self.ui = ui;
        self.panel.window.request_redraw();
    }

    fn toggle_pause(&mut self, fonts: &Fonts) {
        self.paused = !self.paused;
        let command = if self.paused { RecordingCommand::Pause } else { RecordingCommand::Resume };
        let _ = self.commands.send(command);
        self.rebuild(fonts);
    }

    fn panel_input(&mut self, input: PanelInput, fonts: &Fonts) {
        let (mode, action) = panel_step(self.mode, input);
        if action == Some(PanelAction::Cancel) {
            let _ = self.commands.send(RecordingCommand::Cancel);
        }
        if mode != self.mode {
            self.mode = mode;
            self.rebuild(fonts);
        }
    }

    fn redraw(&mut self, fonts: &Fonts) {
        let size = self.panel.size();
        if size.width <= 0 || size.height <= 0 {
            return;
        }
        let mut painter = Painter::new(size, self.scale);
        paint_panel(&mut painter, &self.ui, self.paused, fonts);
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
                        Some(ID_PAUSE) => self.toggle_pause(fonts),
                        Some(ID_STOP) => {
                            let _ = self.commands.send(RecordingCommand::Stop);
                        }
                        Some(ID_CANCEL) => self.panel_input(PanelInput::Discard, fonts),
                        Some(ID_CONFIRM) => self.panel_input(PanelInput::Confirm, fonts),
                        Some(ID_BACK) => self.panel_input(PanelInput::Back, fonts),
                        _ => {}
                    }
                }
            }
            WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key: Key::Named(key), state, .. },
                ..
            } if *state == ElementState::Pressed => match key {
                NamedKey::Escape => self.panel_input(PanelInput::Escape, fonts),
                NamedKey::Enter => self.panel_input(PanelInput::Enter, fonts),
                _ => {}
            },
            _ => {}
        }
    }
}

/// The red ring around `region`: a window whose inside is transparent and
/// click-through, so the recorded area stays visible and usable.
fn open_frame(event_loop: &ActiveEventLoop, region: PixelRect) -> Option<WindowSurface> {
    let mut frame = popup(event_loop, frame_bounds(region, FRAME_THICKNESS))?;
    // Order matters: winit rewrites the extended style when visibility changes,
    // and keeps LAYERED|TRANSPARENT only while cursor hit-testing is off.
    frame.window.set_visible(true);
    let _ = frame.window.set_cursor_hittest(false);
    make_color_keyed(frame.hwnd(), FRAME_KEY);
    paint_frame(&mut frame);
    Some(frame)
}

fn paint_frame(frame: &mut WindowSurface) {
    let size = frame.size();
    let mut pixels = vec![0u32; (size.width.max(0) * size.height.max(0)) as usize];
    render_frame(size, FRAME_THICKNESS, GYOZA.accent.0, FRAME_KEY, &mut pixels);
    frame.present(&pixels);
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::application::ApplicationHandler;
    use winit::event_loop::EventLoop;
    use winit::platform::pump_events::EventLoopExtPumpEvents;
    use winit::platform::windows::EventLoopBuilderExtWindows;

    struct FrameProbe {
        region: PixelRect,
        frame: Option<WindowSurface>,
    }

    impl ApplicationHandler for FrameProbe {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.frame.is_none() {
                self.frame = open_frame(event_loop, self.region);
            }
        }

        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, mut event: WindowEvent) {
            if let Some(frame) = &self.frame {
                frame.keep_on_target(&mut event);
            }
        }
    }

    fn texts() -> &'static gyozarec::i18n::Texts {
        gyozarec::i18n::Language::En.texts()
    }

    #[test]
    fn panel_shows_pause_then_resume() {
        let fonts = Fonts::embedded();
        let icon = |paused| {
            let ui = build_panel_ui(
                Duration::ZERO,
                Duration::from_secs(60),
                paused,
                PanelMode::Normal,
                texts(),
                &fonts,
            );
            ui.widget(ID_PAUSE).map(|w| w.kind.clone())
        };
        assert_eq!(icon(false), Some(WidgetKind::IconButton { icon: Icon::Pause }));
        assert_eq!(icon(true), Some(WidgetKind::IconButton { icon: Icon::Resume }));
    }

    #[test]
    fn panel_asks_before_discarding() {
        let fonts = Fonts::embedded();
        let ui = |mode| {
            build_panel_ui(Duration::ZERO, Duration::from_secs(60), false, mode, texts(), &fonts)
        };
        let normal = ui(PanelMode::Normal);
        let kind = |ui: &Ui, id| ui.widget(id).map(|w| w.kind.clone());
        assert_eq!(kind(&normal, ID_CANCEL), Some(WidgetKind::IconButton { icon: Icon::Discard }));
        assert!(normal.widget(ID_QUESTION).is_none());
        let confirming = ui(PanelMode::Confirming);
        assert!(matches!(
            kind(&confirming, ID_QUESTION),
            Some(WidgetKind::Label { text, .. }) if text == texts().discard_question
        ));
        assert_eq!(
            kind(&confirming, ID_CONFIRM),
            Some(WidgetKind::IconButton { icon: Icon::Discard })
        );
        assert_eq!(kind(&confirming, ID_BACK), Some(WidgetKind::IconButton { icon: Icon::Back }));
    }

    #[test]
    fn panel_size_fits_both_layouts_in_every_language() {
        let fonts = Fonts::embedded();
        let max = Duration::from_secs(3600);
        for lang in gyozarec::i18n::Language::ALL {
            let fonts = Fonts::for_language(lang);
            let t = lang.texts();
            let (w, h) = panel_logical_size(max, t, &fonts);
            for (paused, mode) in [
                (false, PanelMode::Normal),
                (true, PanelMode::Normal),
                (false, PanelMode::Confirming),
            ] {
                let b = build_panel_ui(max, max, paused, mode, t, &fonts).bounds();
                assert!(w >= b.right() + PAD && h >= b.bottom() + PAD, "{lang:?} {mode:?}");
            }
        }
        let _ = fonts;
    }

    /// Draws the panel offscreen into `%TEMP%\\gyozarec-panel-<paused>.png`
    /// for a visual check; no window is opened.
    #[test]
    #[ignore = "desktop"]
    fn render_panel_preview() {
        let fonts = Fonts::embedded();
        let max = Duration::from_secs(300);
        let layouts = [
            ("false", false, PanelMode::Normal),
            ("true", true, PanelMode::Normal),
            ("confirm", false, PanelMode::Confirming),
        ];
        for (name, paused, mode) in layouts {
            let ui = build_panel_ui(Duration::from_secs(7), max, paused, mode, texts(), &fonts);
            let (lw, lh) = panel_logical_size(max, texts(), &fonts);
            let scale = 2.0;
            let (w, h) = ((lw * scale).ceil(), (lh * scale).ceil());
            let size = PixelSize { width: w as i32, height: h as i32 };
            let mut painter = Painter::new(size, scale);
            paint_panel(&mut painter, &ui, paused, &fonts);
            let mut pixels = vec![0u32; (size.width * size.height) as usize];
            painter.to_softbuffer(&mut pixels);
            let img = image::RgbImage::from_fn(w as u32, h as u32, |x, y| {
                let c = pixels[(y * w as u32 + x) as usize];
                image::Rgb([(c >> 16) as u8, (c >> 8) as u8, c as u8])
            });
            img.save(std::env::temp_dir().join(format!("gyozarec-panel-{name}.png"))).unwrap();
        }
    }

    /// The frame must stay transparent inside after it is shown (winit
    /// rewrites the extended style when visibility changes) and sit exactly
    /// around the region, also on a secondary monitor whose DPI may differ
    /// from the primary's.
    #[test]
    #[ignore = "desktop"]
    fn frame_is_transparent_and_exactly_placed() {
        use gyozarec::capture::MonitorProvider;
        let monitors = gyozarec::platform::Win32Monitors.monitors();
        let primary = gyozarec::geometry::PixelPoint { x: 0, y: 0 };
        let target = monitors
            .iter()
            .find(|m| !m.bounds.contains(primary))
            .or_else(|| monitors.first())
            .expect("a monitor");
        let region = PixelRect::new(target.bounds.x + 200, target.bounds.y + 200, 160, 120);
        let mut event_loop = EventLoop::builder().with_any_thread(true).build().unwrap();
        let mut probe = FrameProbe { region, frame: None };
        for _ in 0..5 {
            event_loop.pump_app_events(Some(Duration::from_millis(40)), &mut probe);
        }
        let frame = probe.frame.as_ref().expect("frame window");
        assert_eq!(gyozarec::platform::layered_color_key(frame.hwnd()), Some(FRAME_KEY));
        let expected = frame_bounds(region, FRAME_THICKNESS);
        let pos = frame.window.outer_position().unwrap();
        let size = frame.window.inner_size();
        assert_eq!(PixelRect::new(pos.x, pos.y, size.width as i32, size.height as i32), expected);
    }
}
