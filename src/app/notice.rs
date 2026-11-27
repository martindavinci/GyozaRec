// SPDX-License-Identifier: GPL-3.0-or-later
//! The notification after a capture: thumbnail, file name, click to open;
//! a video's also offers to open its folder.

use super::surface::WindowSurface;
use gyozarec::i18n::Texts;
use gyozarec::pixels::PixelBuffer;
use gyozarec::presentation::{Notice, NoticeKind, notice_rect, shorten_middle};
use gyozarec::uikit::painter::{Fonts, Painter, RectF, measure_text};
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
/// Between "Click to open" and "Open folder".
const SEPARATOR: &str = " — ";

#[derive(Debug, PartialEq, Eq)]
pub enum NoticeAction {
    None,
    Open,
    /// Show the file in its folder.
    Reveal,
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

/// Where "Open folder" goes: after "<open> — " on the hint line.
pub fn reveal_rect(l: &NoticeLayout, open: &str, reveal: &str, fonts: &Fonts) -> RectF {
    let px = theme::current().font_px;
    let x = PAD + measure_text(fonts, &format!("{open}{SEPARATOR}"), px, false).0;
    let (w, h) = measure_text(fonts, reveal, px, false);
    RectF { x, y: l.hint_y, w, h: h.max(LINE) }
}

/// What a click at `(x, y)` does; the close button is handled by its widget.
fn click(l: &NoticeLayout, reveal: Option<RectF>, x: f32, y: f32) -> NoticeAction {
    if l.close.contains(x, y) {
        NoticeAction::None
    } else if reveal.is_some_and(|r| r.contains(x, y)) {
        NoticeAction::Reveal
    } else {
        NoticeAction::Open
    }
}

/// The file name, shortened, with the number of files of a split recording.
fn name_line(name: &str, parts: usize, texts: &Texts) -> String {
    if parts <= 1 {
        return shorten_middle(name, NAME_CHARS);
    }
    let count = texts.notice_parts.replace("{count}", &parts.to_string());
    let room = NAME_CHARS.saturating_sub(count.chars().count() + 3).max(8);
    format!("{} · {count}", shorten_middle(name, room))
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

/// The words of a notification.
#[derive(Clone, Copy)]
struct Lines<'a> {
    name: &'a str,
    hint: &'a str,
    /// "Open folder" and where it goes, on a video's notification.
    reveal: Option<(&'a str, RectF)>,
}

fn paint_notice(
    p: &mut Painter,
    l: &NoticeLayout,
    ui: &Ui,
    thumbnail: &PixelBuffer,
    lines: Lines,
    fonts: &Fonts,
) {
    let Lines { name, hint, reveal } = lines;
    p.clear(theme::current().bg);
    let (w, h) = l.size;
    p.stroke_rect(
        RectF { x: 0.5, y: 0.5, w: w - 1.0, h: h - 1.0 },
        1.0,
        theme::current().disabled_text,
    );
    p.text(fonts, name, PAD, l.name_y, theme::current().font_px, theme::current().text, true);
    p.image(thumbnail, l.thumb);
    let (px, accent) = (theme::current().font_px, theme::current().accent);
    match reveal {
        Some((text, rect)) => {
            p.text(fonts, &format!("{hint}{SEPARATOR}"), PAD, l.hint_y, px, accent, false);
            p.text(fonts, text, rect.x, rect.y, px, accent, false);
            // Underlined: it is a separate link, not part of the hint.
            let y = rect.y + px + 2.0;
            p.line((rect.x, y), (rect.right(), y), 1.0, accent);
        }
        None => p.text(fonts, hint, PAD, l.hint_y, px, accent, false),
    }
    ui.paint(p, fonts, theme::current());
}

pub struct NoticeWindow {
    surface: WindowSurface,
    pub notice: Notice,
    layout: NoticeLayout,
    ui: Ui,
    name: String,
    hint: &'static str,
    /// "Open folder" and where it is, on a video's notification.
    reveal: Option<(&'static str, RectF)>,
    cursor: (f32, f32),
    inside: bool,
}

impl NoticeWindow {
    pub fn open(
        event_loop: &ActiveEventLoop,
        notice: Notice,
        texts: &Texts,
        fonts: &Fonts,
    ) -> Option<NoticeWindow> {
        let l = layout((notice.thumbnail.width(), notice.thumbnail.height()));
        let rect = notice_rect(
            notice.work_area,
            notice.scale * f64::from(gyozarec::uikit::theme::zoom()),
            l.size,
        );
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
        let reveal = (notice.kind == NoticeKind::Video)
            .then(|| (texts.notice_reveal, reveal_rect(&l, hint, texts.notice_reveal, fonts)));
        let name = name_line(&name, notice.parts, texts);
        let ui = close_button(&l);
        Some(NoticeWindow {
            surface,
            notice,
            layout: l,
            ui,
            name,
            hint,
            reveal,
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

    /// The monitor's scale times the interface size.
    fn scale(&self) -> f32 {
        self.notice.scale as f32 * gyozarec::uikit::theme::zoom()
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
            Lines { name: &self.name, hint: self.hint, reveal: self.reveal },
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
                    _ => click(
                        &self.layout,
                        self.reveal.map(|(_, r)| r),
                        self.cursor.0,
                        self.cursor.1,
                    ),
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

    #[test]
    fn open_folder_sits_after_the_open_hint_and_has_its_own_click() {
        let fonts = Fonts::embedded();
        let l = layout((320, 180));
        let r = reveal_rect(&l, "Click to open", "Open folder", &fonts);
        let open_w = measure_text(&fonts, "Click to open", theme::current().font_px, false).0;
        assert!(r.x > PAD + open_w && r.y == l.hint_y && r.w > 0.0);
        assert!(r.right() <= WIDTH - PAD, "fits in the notification");
        let mid = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        assert_eq!(click(&l, Some(r), mid.0, mid.1), NoticeAction::Reveal);
        assert_eq!(click(&l, None, mid.0, mid.1), NoticeAction::Open);
        let thumb = (l.thumb.x + 5.0, l.thumb.y + 5.0);
        assert_eq!(click(&l, Some(r), thumb.0, thumb.1), NoticeAction::Open);
        let close = (l.close.x + 2.0, l.close.y + 2.0);
        assert_eq!(click(&l, Some(r), close.0, close.1), NoticeAction::None);
    }

    #[test]
    fn open_folder_fits_in_every_language() {
        let l = layout((320, 180));
        for lang in gyozarec::i18n::Language::ALL {
            let texts = lang.texts();
            let fonts = Fonts::for_language(lang);
            let r = reveal_rect(&l, texts.notice_open, texts.notice_reveal, &fonts);
            assert!(r.right() <= WIDTH - PAD, "{lang:?}: ends at {}", r.right());
        }
    }

    #[test]
    fn name_line_counts_the_parts_of_a_split_recording() {
        let texts = gyozarec::i18n::Language::It.texts();
        assert_eq!(name_line("clip.mp4", 1, texts), "clip.mp4");
        assert_eq!(name_line("clip.mp4", 3, texts), "clip.mp4 · 3 parti");
        let long = "GyozaRec_2026-11-27_18-30-12_abcdefgh.mp4";
        assert!(name_line(long, 12, texts).chars().count() <= NAME_CHARS + 2);
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
        let name = name_line("GyozaRec_2026-10-25_18-30-12.mp4", 3, texts);
        let reveal = Some((
            texts.notice_reveal,
            reveal_rect(&l, texts.notice_open, texts.notice_reveal, &fonts),
        ));
        paint_notice(
            &mut p,
            &l,
            &close_button(&l),
            &thumb,
            Lines { name: &name, hint: texts.notice_open, reveal },
            &fonts,
        );
        let mut pixels = vec![0u32; (size.width * size.height) as usize];
        p.to_softbuffer(&mut pixels);
        let img = image::RgbImage::from_fn(size.width as u32, size.height as u32, |x, y| {
            let c = pixels[(y * size.width as u32 + x) as usize];
            image::Rgb([(c >> 16) as u8, (c >> 8) as u8, c as u8])
        });
        img.save(std::env::temp_dir().join("gyozarec-notice.png")).unwrap();
    }
}
