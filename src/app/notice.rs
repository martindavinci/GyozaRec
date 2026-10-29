//! The notification after a capture: thumbnail, file name, click to open.

use super::surface::WindowSurface;
use gyozarec::i18n::Texts;
use gyozarec::pixels::PixelBuffer;
use gyozarec::presentation::{Notice, NoticeKind, notice_rect, shorten_middle};
use gyozarec::uikit::painter::{Fonts, Painter, RectF};
use gyozarec::uikit::theme;
use gyozarec::uikit::widgets::{Icon, Ui, Widget, WidgetKind};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{Window, WindowId, WindowLevel};

const PAD: f32 = 12.0;
const WIDTH: f32 = 272.0;
/// Largest thumbnail, in logical pixels.
const THUMB: (f32, f32) = (240.0, 135.0);
const LINE: f32 = 18.0;
const NAME_CHARS: usize = 30;
const ID_CLOSE: u32 = 1;

#[derive(Debug, PartialEq, Eq)]
pub enum NoticeAction {
    None,
    Open,
    Close,
    /// The pointer entered (`true`) or left the notification.
    Hover(bool),
}

/// Where things go in a notification, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoticeLayout {
    pub size: (f32, f32),
    pub thumb: RectF,
    pub close: RectF,
    pub name_y: f32,
    pub hint_y: f32,
}

pub fn layout(thumbnail: (i32, i32)) -> NoticeLayout {
    let (w, h) = (thumbnail.0.max(1) as f32, thumbnail.1.max(1) as f32);
    let k = (THUMB.0 / w).min(THUMB.1 / h).min(1.0);
    let (tw, th) = ((w * k).round(), (h * k).round());
    let c = theme::current().control_height;
    let close = RectF { x: WIDTH - PAD - c, y: PAD, w: c, h: c };
    let name_y = PAD + (c - LINE) / 2.0;
    let thumb = RectF { x: (WIDTH - tw) / 2.0, y: PAD + c + 6.0, w: tw, h: th };
    let hint_y = thumb.bottom() + 8.0;
    NoticeLayout { size: (WIDTH, hint_y + LINE + PAD), thumb, close, name_y, hint_y }
}

fn close_button(l: &NoticeLayout) -> Ui {
    let close = Widget {
        id: ID_CLOSE,
        kind: WidgetKind::IconButton { icon: Icon::Cancel },
        rect: l.close,
        enabled: true,
    };
    Ui::new(vec![close])
}

fn paint_notice(
    p: &mut Painter,
    l: &NoticeLayout,
    ui: &Ui,
    thumbnail: &PixelBuffer,
    name: &str,
    hint: &str,
    fonts: &Fonts,
) {
    p.clear(theme::current().bg);
    let (w, h) = l.size;
    p.stroke_rect(
        RectF { x: 0.5, y: 0.5, w: w - 1.0, h: h - 1.0 },
        1.0,
        theme::current().disabled_text,
    );
    p.text(fonts, name, PAD, l.name_y, theme::current().font_px, theme::current().text, true);
    p.image(thumbnail, l.thumb);
    p.text(fonts, hint, PAD, l.hint_y, theme::current().font_px, theme::current().accent, false);
    ui.paint(p, fonts, theme::current());
}

pub struct NoticeWindow {
    surface: WindowSurface,
    pub notice: Notice,
    layout: NoticeLayout,
    ui: Ui,
    name: String,
    hint: &'static str,
    cursor: (f32, f32),
    inside: bool,
}

impl NoticeWindow {
    pub fn open(
        event_loop: &ActiveEventLoop,
        notice: Notice,
        texts: &Texts,
    ) -> Option<NoticeWindow> {
        let l = layout((notice.thumbnail.width(), notice.thumbnail.height()));
        let rect = notice_rect(notice.work_area, notice.scale, l.size);
        let attrs = Window::default_attributes()
            .with_title("GyozaRec")
            .with_decorations(false)
            .with_resizable(false)
            .with_visible(false)
            .with_active(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_skip_taskbar(true);
        let window = event_loop.create_window(attrs).ok()?;
        let surface = WindowSurface::pinned(window, rect);
        gyozarec::platform::exclude_from_capture(surface.hwnd());
        // Shown without taking the focus from what the user is doing.
        gyozarec::platform::show_without_activating(surface.hwnd());
        let name =
            notice.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let hint = match notice.kind {
            NoticeKind::Image => texts.notice_edit,
            NoticeKind::Video => texts.notice_open,
        };
        let ui = close_button(&l);
        Some(NoticeWindow {
            surface,
            notice,
            layout: l,
            ui,
            name: shorten_middle(&name, NAME_CHARS),
            hint,
            cursor: (-1.0, -1.0),
            inside: false,
        })
    }

    pub fn id(&self) -> WindowId {
        self.surface.window.id()
    }

    pub fn surface(&self) -> &WindowSurface {
        &self.surface
    }

    fn scale(&self) -> f32 {
        self.notice.scale as f32
    }

    fn redraw(&mut self, fonts: &Fonts) {
        let size = self.surface.size();
        if size.width <= 0 || size.height <= 0 {
            return;
        }
        let mut p = Painter::new(size, self.scale());
        paint_notice(
            &mut p,
            &self.layout,
            &self.ui,
            &self.notice.thumbnail,
            &self.name,
            self.hint,
            fonts,
        );
        let mut pixels = vec![0u32; (size.width * size.height) as usize];
        p.to_softbuffer(&mut pixels);
        self.surface.present(&pixels);
    }

    pub fn handle(&mut self, event: &WindowEvent, fonts: &Fonts) -> NoticeAction {
        match event {
            WindowEvent::RedrawRequested => self.redraw(fonts),
            WindowEvent::CursorMoved { position, .. } => {
                let s = self.scale();
                self.cursor = (position.x as f32 / s, position.y as f32 / s);
                if self.ui.pointer_moved(self.cursor.0, self.cursor.1) {
                    self.surface.window.request_redraw();
                }
                if !self.inside {
                    self.inside = true;
                    return NoticeAction::Hover(true);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if self.ui.pointer_left() {
                    self.surface.window.request_redraw();
                }
                self.inside = false;
                return NoticeAction::Hover(false);
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                self.surface.window.request_redraw();
                if *state == ElementState::Pressed {
                    self.ui.pointer_down(self.cursor.0, self.cursor.1);
                    return NoticeAction::None;
                }
                return match self.ui.pointer_up(self.cursor.0, self.cursor.1) {
                    Some(ID_CLOSE) => NoticeAction::Close,
                    _ if !self.layout.close.contains(self.cursor.0, self.cursor.1) => {
                        NoticeAction::Open
                    }
                    _ => NoticeAction::None,
                };
            }
            _ => {}
        }
        NoticeAction::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyozarec::geometry::{PixelRect, PixelSize};

    #[test]
    fn layout_fits_the_thumbnail_without_enlarging_it() {
        let wide = layout((320, 90));
        assert_eq!((wide.thumb.w, wide.thumb.h), (240.0, 68.0));
        let small = layout((100, 50));
        assert_eq!((small.thumb.w, small.thumb.h), (100.0, 50.0));
        assert!(small.size.1 < wide.size.1 + 1.0 && small.hint_y > small.thumb.bottom());
        assert!(wide.close.right() <= WIDTH - PAD + 0.01);
    }

    /// Draws a notification offscreen into `%TEMP%\gyozarec-notice.png`.
    #[test]
    #[ignore = "desktop"]
    fn render_notice_preview() {
        let fonts = Fonts::embedded();
        let thumb = PixelBuffer::solid(PixelRect::new(0, 0, 320, 180), 60, 140, 230);
        let l = layout((320, 180));
        let size = PixelSize { width: l.size.0 as i32, height: l.size.1 as i32 };
        let mut p = Painter::new(size, 1.0);
        let texts = gyozarec::i18n::Language::En.texts();
        let name = shorten_middle("GyozaRec_2026-10-25_18-30-12.png", NAME_CHARS);
        paint_notice(&mut p, &l, &close_button(&l), &thumb, &name, texts.notice_edit, &fonts);
        let mut pixels = vec![0u32; (size.width * size.height) as usize];
        p.to_softbuffer(&mut pixels);
        let img = image::RgbImage::from_fn(size.width as u32, size.height as u32, |x, y| {
            let c = pixels[(y * size.width as u32 + x) as usize];
            image::Rgb([(c >> 16) as u8, (c >> 8) as u8, c as u8])
        });
        img.save(std::env::temp_dir().join("gyozarec-notice.png")).unwrap();
    }
}
