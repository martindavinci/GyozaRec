//! Makes `docs/screenshots/editor.png`: a real editor window with a few
//! annotations, and a label for every button around it. Run on purpose
//! only (it opens a window for two seconds):
//! `cargo test --bin gyozarec readme_editor_screenshot -- --ignored`

use super::AppEvent;
use super::editor::{EditorSettings, EditorWindow};
use gyozarec::capture::ScreenGrabber;
use gyozarec::editor::{Annotation, PALETTE};
use gyozarec::geometry::{PixelPoint, PixelRect, PixelSize};
use gyozarec::i18n::Language;
use gyozarec::pixels::PixelBuffer;
use gyozarec::uikit::painter::{Fonts, Painter, RectF, measure_text};
use gyozarec::uikit::theme::{Color, GYOZA};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::windows::EventLoopBuilderExtWindows;
use winit::window::{WindowId, WindowLevel};

/// A pizza and a menu: something friendly to annotate.
fn scene(fonts: &Fonts) -> PixelBuffer {
    let base = PixelBuffer::solid(PixelRect::new(0, 0, 1000, 560), 0xEC, 0xF8, 0xFF);
    let mut p = Painter::from_buffer(&base);
    p.text(fonts, "Pizzeria Gyoza", 40.0, 28.0, 40.0, Color(0x7A3E1D), true);
    // The pizza.
    let (cx, cy) = (300.0, 330.0);
    p.fill_circle(cx, cy, 190.0, Color(0xD99A4E));
    p.fill_circle(cx, cy, 168.0, Color(0xC0392B));
    for (dx, dy, r) in [
        (-60.0, -50.0, 46.0),
        (50.0, -70.0, 40.0),
        (70.0, 40.0, 50.0),
        (-40.0, 70.0, 44.0),
        (0.0, 0.0, 38.0),
    ] {
        p.fill_circle(cx + dx, cy + dy, r, Color(0xF7D774));
    }
    for (dx, dy) in
        [(-95.0, -10.0), (10.0, -110.0), (105.0, -20.0), (20.0, 105.0), (-70.0, 95.0), (45.0, 15.0)]
    {
        p.fill_circle(cx + dx, cy + dy, 22.0, Color(0x8E1B1B));
    }
    for (dx, dy) in [(-20.0, -60.0), (-115.0, 50.0), (110.0, 85.0)] {
        p.fill_circle(cx + dx, cy + dy, 12.0, Color(0x3E8E41));
        p.fill_circle(cx + dx + 12.0, cy + dy + 6.0, 10.0, Color(0x3E8E41));
    }
    // The menu.
    p.fill_rounded_rect(RectF { x: 600.0, y: 110.0, w: 360.0, h: 400.0 }, 14.0, Color(0xFFFFFF));
    p.text(fonts, "Menu", 630.0, 128.0, 30.0, Color(0x7A3E1D), true);
    let items = [("Margherita", "8"), ("Diavola", "9"), ("Gyoza", "6"), ("Tiramisu", "5")];
    for (i, (name, price)) in items.iter().enumerate() {
        let y = 190.0 + i as f32 * 52.0;
        p.text(fonts, name, 630.0, y, 26.0, Color(0x333333), false);
        p.text(fonts, &format!("€ {price}"), 880.0, y, 26.0, Color(0x333333), false);
    }
    p.text(fonts, "Wi-Fi: pizza1234", 630.0, 430.0, 24.0, Color(0x555555), false);
    p.into_buffer(PixelPoint { x: 0, y: 0 })
}

fn annotations() -> Vec<Annotation> {
    let circle: Vec<(f32, f32)> = (0..=40)
        .map(|i| {
            let a = i as f32 / 40.0 * std::f32::consts::TAU;
            (190.0 + 34.0 * a.cos(), 383.0 + 30.0 * a.sin())
        })
        .collect();
    vec![
        Annotation::Arrow {
            from: (520.0, 150.0),
            to: (380.0, 245.0),
            color: PALETTE[0],
            width: 7.0,
        },
        Annotation::Text {
            at: (430.0, 92.0),
            text: "Tonight!".into(),
            color: PALETTE[0],
            px: 34.0,
        },
        Annotation::Stroke {
            points: vec![(626.0, 207.0), (790.0, 207.0)],
            color: PALETTE[1],
            width: 9.0,
            highlight: true,
        },
        // Pixelate first, so the rectangle stays sharp over it.
        Annotation::Pixelate { a: (870.0, 186.0), b: (945.0, 400.0), block: 12.0 },
        Annotation::Rectangle {
            a: (620.0, 290.0),
            b: (940.0, 334.0),
            color: PALETTE[3],
            width: 5.0,
        },
        Annotation::Stroke { points: circle, color: PALETTE[2], width: 5.0, highlight: false },
        Annotation::Redact { a: (683.0, 428.0), b: (860.0, 462.0), color: PALETTE[4] },
    ]
}

/// The widget ids the labels point at, with their text.
fn labels() -> Vec<(u32, &'static str)> {
    use super::editor::bar_ids as ids;
    let tools = ["Arrow", "Rectangle", "Pen", "Highlighter", "Text", "Pixelate", "Redact", "Crop"];
    let mut out: Vec<(u32, &str)> =
        tools.iter().enumerate().map(|(i, t)| (ids::TOOL + i as u32, *t)).collect();
    out.extend([
        (ids::SWATCH + 2, "Colors"),
        (ids::SIZE, "Size"),
        (ids::UNDO, "Undo"),
        (ids::REDO, "Redo"),
        (ids::NEW, "Capture again"),
        (ids::OPEN, "Edit another image"),
        (ids::COPY, "Copy to clipboard"),
        (ids::SAVE_AS, "Save as PNG or JPG"),
        (ids::SAVE, "Save the capture"),
    ]);
    out
}

/// Labels on two alternating rows, in button order, each row pushed apart so
/// labels keep `GAP` between them and stay inside `width`.
fn place(targets: &[(f32, f32)], width: f32) -> Vec<(RectF, usize)> {
    const GAP: f32 = 18.0;
    let mut placed: Vec<(RectF, usize)> = targets
        .iter()
        .enumerate()
        .map(|(i, &(x, w))| (RectF { x: x - w / 2.0, y: 0.0, w, h: 0.0 }, i % 2))
        .collect();
    for row in 0..2 {
        let idx: Vec<usize> = (0..placed.len()).filter(|i| placed[*i].1 == row).collect();
        let mut prev_right = 4.0 - GAP;
        for &i in &idx {
            let r = &mut placed[i].0;
            r.x = r.x.max(prev_right + GAP);
            prev_right = r.right();
        }
        let mut next_left = width - 4.0 + GAP;
        for &i in idx.iter().rev() {
            let r = &mut placed[i].0;
            r.x = r.x.min(next_left - GAP - r.w);
            next_left = r.x;
        }
    }
    placed
}

struct Shot {
    proxy: EventLoopProxy<AppEvent>,
    editor: Option<EditorWindow>,
    opened: Option<Instant>,
    fonts: Arc<Fonts>,
}

impl ApplicationHandler<AppEvent> for Shot {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.editor.is_some() {
            return;
        }
        let image = Arc::new(scene(&self.fonts));
        let settings =
            EditorSettings { copy_to_clipboard: false, jpeg_quality: 90, ask_to_save: true };
        let path = std::path::PathBuf::from("2026-10-26_20-15-00.png");
        let mut editor = EditorWindow::open(
            event_loop,
            image,
            path,
            true,
            settings,
            Language::En.texts(),
            self.fonts.clone(),
            None,
            1.0,
            self.proxy.clone(),
        )
        .expect("editor window");
        for a in annotations() {
            editor.add_for_screenshot(a);
        }
        editor.window().set_window_level(WindowLevel::AlwaysOnTop);
        self.editor = Some(editor);
        self.opened = Some(Instant::now());
        event_loop
            .set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(100)));
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let Some(e) = &mut self.editor {
            e.handle(&event);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(opened) = self.opened else { return };
        if opened.elapsed() < Duration::from_secs(2) {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(100),
            ));
            return;
        }
        if let Some(e) = self.editor.take() {
            compose(&e, &self.fonts);
        }
        event_loop.exit();
    }
}

/// Grabs the window with its title bar and draws the labels above it.
fn compose(editor: &EditorWindow, fonts: &Fonts) {
    let window = editor.window();
    let s = window.scale_factor() as f32;
    let frame = frame_bounds(editor.hwnd());
    let shot = gyozarec::platform::GdiGrabber.grab(frame, false).expect("grab");
    let client = window.inner_position().expect("client position");
    let (ox, oy) = ((client.x - frame.x) as f32, (client.y - frame.y) as f32);
    let px = 13.0;
    let mut targets = Vec::new();
    let mut tops = Vec::new();
    for (id, text) in labels() {
        let r = editor.bar_rect(id).expect("button");
        let x = ox + (r.x + r.w / 2.0) * s;
        tops.push((x, oy + r.y * s));
        let w = measure_text(fonts, text, px, true).0 * s + 12.0 * s;
        targets.push((x, w));
    }
    let side = 24.0 * s;
    let width = frame.width as f32 + 2.0 * side;
    let placed = place(&targets.iter().map(|(x, w)| (x + side, *w)).collect::<Vec<_>>(), width);
    let rows = placed.iter().map(|(_, r)| *r).max().unwrap_or(0) + 1;
    let row_h = 30.0 * s;
    let top = 16.0 * s + rows as f32 * row_h + 18.0 * s;
    let size = PixelSize { width: width as i32, height: (top + frame.height as f32 + side) as i32 };
    let mut p = Painter::new(size, 1.0);
    p.clear(GYOZA.bg);
    p.image(&shot, RectF { x: side, y: top, w: frame.width as f32, h: frame.height as f32 });
    let boxes: Vec<(RectF, (f32, f32), &str)> = placed
        .iter()
        .zip(labels())
        .zip(&tops)
        .map(|(((label, row), (_, text)), (x, y))| {
            let y_box = top - 18.0 * s - (*row as f32 + 1.0) * row_h + 6.0 * s;
            (RectF { x: label.x, y: y_box, w: label.w, h: 22.0 * s }, (x + side, y + top), text)
        })
        .collect();
    // Lines first, so every label sits on top of them.
    for (r, (tx, ty), _) in &boxes {
        p.line((r.x + r.w / 2.0, r.bottom()), (*tx, ty + 2.0 * s), 1.5 * s, GYOZA.accent);
        p.fill_circle(*tx, ty + 2.0 * s, 3.0 * s, GYOZA.accent);
    }
    for (r, _, text) in &boxes {
        p.fill_rounded_rect(*r, 6.0 * s, GYOZA.accent);
        let tw = measure_text(fonts, text, px * s, true).0;
        p.text(fonts, text, r.x + (r.w - tw) / 2.0, r.y + 3.0 * s, px * s, Color(0xFFFFFF), true);
    }
    let mut pixels = vec![0u32; (size.width * size.height) as usize];
    p.to_softbuffer(&mut pixels);
    let img = image::RgbImage::from_fn(size.width as u32, size.height as u32, |x, y| {
        let c = pixels[(y * size.width as u32 + x) as usize];
        image::Rgb([(c >> 16) as u8, (c >> 8) as u8, c as u8])
    });
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/screenshots/editor.png");
    img.save(&out).expect("save");
    eprintln!("wrote {} ({}×{})", out.display(), size.width, size.height);
}

/// The visible window rectangle, title bar included, without resize borders.
fn frame_bounds(hwnd: isize) -> PixelRect {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
    let mut r = RECT::default();
    // SAFETY: a live window of this thread and a RECT-sized out parameter.
    unsafe {
        DwmGetWindowAttribute(
            HWND(hwnd as *mut _),
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut r as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32,
        )
        .expect("frame bounds");
    }
    PixelRect::new(r.left, r.top, r.right - r.left, r.bottom - r.top)
}

#[test]
#[ignore = "desktop"]
fn readme_editor_screenshot() {
    let event_loop =
        EventLoop::<AppEvent>::with_user_event().with_any_thread(true).build().expect("event loop");
    let mut shot = Shot {
        proxy: event_loop.create_proxy(),
        editor: None,
        opened: None,
        fonts: Arc::new(Fonts::embedded()),
    };
    event_loop.run_app(&mut shot).expect("run");
}

#[test]
fn labels_on_a_row_keep_apart_and_in_order() {
    let targets: Vec<(f32, f32)> = (0..17).map(|i| (40.0 + i as f32 * 60.0, 90.0)).collect();
    let placed = place(&targets, 1100.0);
    for row in 0..2 {
        let boxes: Vec<&RectF> = placed.iter().filter(|(_, r)| *r == row).map(|(b, _)| b).collect();
        for w in boxes.windows(2) {
            assert!(w[1].x >= w[0].right() + 17.9, "{:?}", w);
        }
        assert!(boxes.iter().all(|b| b.x >= 4.0 && b.right() <= 1096.01));
    }
}
