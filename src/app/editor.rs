//! The annotation editor window: a bar of tools on top, the capture below.

use super::AppEvent;
use super::surface::WindowSurface;
use gyozarec::capture::ClipboardService;
use gyozarec::editor::{
    Annotation, Document, PALETTE, Point, SIZES, Size, TOOLS, Tool, View, draw, pixel_block,
};
use gyozarec::geometry::{PixelRect, PixelSize};
use gyozarec::i18n::Texts;
use gyozarec::pixels::PixelBuffer;
use gyozarec::platform::{SaveChoice, Win32Clipboard, ask_save_changes, show_error_box};
use gyozarec::uikit::painter::{Fonts, Painter, RectF, measure_text};
use gyozarec::uikit::theme::{Color, GYOZA};
use gyozarec::uikit::widgets::{Icon, Ui, Widget, WidgetKind, layout_row};
use muda::{CheckMenuItem, ContextMenu, Menu};
use std::path::PathBuf;
use std::sync::Arc;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

const PAD: f32 = 8.0;
const GROUP_GAP: f32 = 14.0;
/// Space around the picture inside the canvas.
const MARGIN: f32 = 12.0;
const CANVAS_BG: Color = Color(0xEADBC4);
const ID_TOOL: u32 = 100;
const ID_SWATCH: u32 = 200;
const ID_SIZE: u32 = 300;
const ID_UNDO: u32 = 301;
const ID_REDO: u32 = 302;
const ID_COPY: u32 = 303;
const ID_SAVE_AS: u32 = 304;
const ID_SAVE: u32 = 305;
/// Menu ids: `editor:<window id>:size:<index>`.
pub const MENU_PREFIX: &str = "editor:";

/// What the editor needs from the settings.
#[derive(Clone, Copy, Debug)]
pub struct EditorSettings {
    pub copy_to_clipboard: bool,
    pub jpeg_quality: u8,
}

#[derive(Debug, PartialEq, Eq)]
pub enum EditorResult {
    Open,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorCommand {
    Undo,
    Redo,
    Save,
    SaveAs,
    Copy,
    Close,
}

/// Keyboard commands; while text is being typed, Ctrl+C and Esc belong to it.
pub fn command_for(key: &Key, mods: ModifiersState, typing: bool) -> Option<EditorCommand> {
    if let Key::Named(NamedKey::Escape) = key {
        return (!typing).then_some(EditorCommand::Close);
    }
    if !mods.control_key() || mods.alt_key() {
        return None;
    }
    let Key::Character(c) = key else { return None };
    match (c.to_lowercase().as_str(), mods.shift_key()) {
        ("z", false) => Some(EditorCommand::Undo),
        ("z", true) | ("y", _) => Some(EditorCommand::Redo),
        ("s", false) => Some(EditorCommand::Save),
        ("s", true) => Some(EditorCommand::SaveAs),
        ("c", _) if !typing => Some(EditorCommand::Copy),
        _ => None,
    }
}

fn icon_of(tool: Tool) -> Icon {
    match tool {
        Tool::Arrow => Icon::Arrow,
        Tool::Rectangle => Icon::Rectangle,
        Tool::Pen => Icon::Pen,
        Tool::Highlighter => Icon::Highlighter,
        Tool::Text => Icon::Text,
        Tool::Pixelate => Icon::Pixelate,
        Tool::Redact => Icon::Redact,
        Tool::Crop => Icon::Crop,
    }
}

/// Logical inner size for a new editor: the picture at one image pixel per
/// screen pixel, within 80 % of the work area, at least 720×420 when it fits.
pub fn initial_size(image: PixelSize, work_area: Option<PixelRect>, scale: f64) -> (f32, f32) {
    let s = scale.max(0.5) as f32;
    let bar = 2.0 * PAD + GYOZA.control_height;
    let content =
        (image.width as f32 / s + 2.0 * MARGIN, image.height as f32 / s + 2.0 * MARGIN + bar);
    let (aw, ah) = work_area.map_or((1280.0, 800.0), |a| (a.width as f32 / s, a.height as f32 / s));
    let min = (720f32.min(aw * 0.9), 420f32.min(ah * 0.9));
    let max = ((aw * 0.8).max(min.0), (ah * 0.8).max(min.1));
    (content.0.clamp(min.0, max.0).round(), content.1.clamp(min.1, max.1).round())
}

/// What the bar shows.
struct BarState {
    tool: Tool,
    color: usize,
    size: Size,
    can_undo: bool,
    can_redo: bool,
    saving: bool,
}

/// The tool bar for a window `width` logical units wide; the Copy / Save
/// group moves to a second row when it does not fit beside the tools.
fn build_bar(width: f32, s: &BarState, texts: &Texts, fonts: &Fonts) -> Ui {
    let mut x = PAD;
    let mut widgets: Vec<Widget> = Vec::new();
    let group = |items: Vec<(u32, WidgetKind)>, x: &mut f32, widgets: &mut Vec<Widget>| {
        let row = layout_row(items, *x, PAD, 4.0, fonts, &GYOZA);
        *x = row.last().map_or(*x, |w| w.rect.right()) + GROUP_GAP;
        widgets.extend(row);
    };
    let tools = TOOLS.iter().enumerate().map(|(i, t)| {
        (ID_TOOL + i as u32, WidgetKind::ToolButton { icon: icon_of(*t), selected: *t == s.tool })
    });
    group(tools.collect(), &mut x, &mut widgets);
    let swatches = PALETTE.iter().enumerate().map(|(i, c)| {
        (ID_SWATCH + i as u32, WidgetKind::Swatch { color: c.color(), selected: i == s.color })
    });
    group(swatches.collect(), &mut x, &mut widgets);
    let size_label = texts.editor_sizes[SIZES.iter().position(|z| *z == s.size).unwrap_or(1)];
    let history = vec![
        (ID_SIZE, WidgetKind::Button { label: size_label.into(), dropdown: true }),
        (ID_UNDO, WidgetKind::IconButton { icon: Icon::Undo }),
        (ID_REDO, WidgetKind::IconButton { icon: Icon::Redo }),
    ];
    group(history, &mut x, &mut widgets);
    for w in &mut widgets {
        w.enabled = match w.id {
            ID_UNDO => s.can_undo,
            ID_REDO => s.can_redo,
            _ => true,
        };
    }
    let button = |label: &str| WidgetKind::Button { label: label.into(), dropdown: false };
    let files = vec![
        (ID_COPY, button(texts.editor_copy)),
        (ID_SAVE_AS, button(texts.editor_save_as)),
        (ID_SAVE, button(texts.editor_save)),
    ];
    let mut right = layout_row(files, 0.0, PAD, 4.0, fonts, &GYOZA);
    let group_w = right.last().map_or(0.0, |w| w.rect.right());
    let left_end = x - GROUP_GAP;
    let (dx, dy) = if width - PAD - group_w >= left_end + GROUP_GAP {
        (width - PAD - group_w, 0.0)
    } else {
        (PAD, GYOZA.control_height + 6.0)
    };
    for w in &mut right {
        w.rect.x += dx;
        w.rect.y += dy;
        w.enabled = !s.saving;
    }
    widgets.extend(right);
    Ui::new(widgets)
}

/// A drag on the canvas, in image pixels.
#[derive(Clone, Debug, PartialEq)]
pub enum Drag {
    Shape { start: Point, current: Point },
    Stroke(Vec<Point>),
}

/// What a finished drag does.
#[derive(Clone, Debug, PartialEq)]
pub enum DragEnd {
    Add(Annotation),
    Crop(Point, Point),
    Nothing,
}

/// Turns a drag into an annotation; shapes too small to see add nothing.
pub fn finish_drag(drag: &Drag, tool: Tool, color: usize, size: Size, image: PixelSize) -> DragEnd {
    let color = PALETTE[color.min(PALETTE.len() - 1)];
    match drag {
        Drag::Stroke(points) if !points.is_empty() => DragEnd::Add(Annotation::Stroke {
            points: points.clone(),
            color,
            width: size.stroke(),
            highlight: tool == Tool::Highlighter,
        }),
        Drag::Stroke(_) => DragEnd::Nothing,
        Drag::Shape { start, current } => {
            let (w, h) = ((current.0 - start.0).abs(), (current.1 - start.1).abs());
            let (a, b) = (*start, *current);
            match tool {
                Tool::Arrow if w.hypot(h) >= 2.0 => {
                    DragEnd::Add(Annotation::Arrow { from: a, to: b, color, width: size.stroke() })
                }
                Tool::Rectangle if w >= 2.0 && h >= 2.0 => {
                    DragEnd::Add(Annotation::Rectangle { a, b, color, width: size.stroke() })
                }
                Tool::Pixelate if w >= 4.0 && h >= 4.0 => DragEnd::Add(Annotation::Pixelate {
                    a,
                    b,
                    block: pixel_block(image.width, image.height, size),
                }),
                Tool::Redact if w >= 2.0 && h >= 2.0 => {
                    DragEnd::Add(Annotation::Redact { a, b, color })
                }
                Tool::Crop => DragEnd::Crop(a, b),
                _ => DragEnd::Nothing,
            }
        }
    }
}

/// The picture as shown: the final image, and its copy fitted to the
/// canvas, each rebuilt only when what it depends on changes.
#[derive(Default)]
pub struct EditorCanvas {
    full: Option<(u64, PixelBuffer)>,
    /// (version, physical width, height), and the fitted copy when it
    /// differs in size from the final image.
    fitted: Option<((u64, i32, i32), Option<PixelBuffer>)>,
    pub rebuilds: usize,
}

impl EditorCanvas {
    /// The image to show, the view placing it, and where it goes (logical).
    pub fn frame(
        &mut self,
        doc: &Document,
        canvas: RectF,
        scale: f32,
        fonts: &Fonts,
    ) -> (&PixelBuffer, View, RectF) {
        let version = doc.version();
        let visible = doc.visible_rect();
        let view = View::fit_at_most(visible, canvas, 1.0 / scale.max(0.1));
        let (x, y) = view.to_canvas((visible.x as f32, visible.y as f32));
        let dest = RectF {
            x,
            y,
            w: visible.width as f32 * view.scale,
            h: visible.height as f32 * view.scale,
        };
        let physical =
            (((dest.w * scale).round() as i32).max(1), ((dest.h * scale).round() as i32).max(1));
        let mut rebuilt = false;
        if self.full.as_ref().is_none_or(|(v, _)| *v != version) {
            self.full = Some((version, doc.flatten(fonts)));
            self.fitted = None;
            rebuilt = true;
        }
        let key = (version, physical.0, physical.1);
        if self.fitted.as_ref().is_none_or(|(k, _)| *k != key) {
            let full = &self.full.as_ref().expect("built above").1;
            let size = PixelSize { width: physical.0, height: physical.1 };
            let copy = (full.bounds.size() != size).then(|| full.resized(size));
            self.fitted = Some((key, copy));
            rebuilt = true;
        }
        self.rebuilds += usize::from(rebuilt);
        let full = &self.full.as_ref().expect("built above").1;
        let shown = self.fitted.as_ref().and_then(|(_, f)| f.as_ref()).unwrap_or(full);
        (shown, view, dest)
    }
}

/// Text being typed, not yet in the document.
struct TextBox {
    at: Point,
    text: String,
    /// What the input method is composing.
    preedit: String,
}

/// What a finished save leads to.
#[derive(Debug, PartialEq)]
pub enum SaveEnd {
    Close,
    /// Edits made during the save still need saving before closing.
    SaveAgain(PathBuf),
    /// Stay open; on success the window now works on `adopt`.
    Stay {
        adopt: Option<PathBuf>,
    },
}

/// The save running on its own thread, if any.
#[derive(Default)]
pub struct SaveTracker {
    /// Target and whether to close once it is written.
    running: Option<(PathBuf, bool)>,
}

impl SaveTracker {
    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// Starts a save unless one runs; a running save only learns to close after.
    pub fn begin(&mut self, path: PathBuf, close_after: bool) -> bool {
        if let Some((_, close)) = &mut self.running {
            *close |= close_after;
            return false;
        }
        self.running = Some((path, close_after));
        true
    }

    /// The save ended; `still_dirty` tells whether edits came in meanwhile.
    pub fn finish(&mut self, ok: bool, still_dirty: bool) -> SaveEnd {
        match self.running.take() {
            Some((path, true)) if ok && still_dirty => SaveEnd::SaveAgain(path),
            Some((_, true)) if ok => SaveEnd::Close,
            Some((path, false)) if ok => SaveEnd::Stay { adopt: Some(path) },
            _ => SaveEnd::Stay { adopt: None },
        }
    }
}

/// Paints the editor: bar, canvas, the shape being drawn and the text box.
#[allow(clippy::too_many_arguments)]
fn paint_editor(
    p: &mut Painter,
    ui: &Ui,
    canvas: &mut EditorCanvas,
    doc: &Document,
    preview: Option<&Annotation>,
    selection: Option<RectF>,
    text: Option<(&Annotation, f32)>,
    area: RectF,
    fonts: &Fonts,
) -> View {
    p.clear(GYOZA.bg);
    p.fill_rounded_rect(area, 0.0, CANVAS_BG);
    let inner = RectF {
        x: area.x + MARGIN,
        y: area.y + MARGIN,
        w: (area.w - 2.0 * MARGIN).max(1.0),
        h: (area.h - 2.0 * MARGIN).max(1.0),
    };
    let (image, view, dest) = canvas.frame(doc, inner, p.scale(), fonts);
    p.image(image, dest);
    if let Some(a) = preview {
        draw(p, a, &view, fonts);
    }
    if let Some(r) = selection {
        p.dashed_rect(r, GYOZA.text);
    }
    if let Some((a, caret_px)) = text {
        draw(p, a, &view, fonts);
        if let Annotation::Text { at, text, px, color } = a {
            let (x, y) = view.to_canvas(*at);
            let w = measure_text(fonts, text, px * view.scale, true).0;
            let h = (px * view.scale).max(caret_px);
            p.line((x + w + 1.0, y), (x + w + 1.0, y + h), 1.5, color.color());
        }
    }
    ui.paint(p, fonts, &GYOZA);
    view
}

pub struct EditorWindow {
    surface: WindowSurface,
    doc: Document,
    path: PathBuf,
    settings: EditorSettings,
    texts: &'static Texts,
    fonts: Arc<Fonts>,
    ui: Ui,
    tool: Tool,
    color: usize,
    size: Size,
    drag: Option<Drag>,
    text: Option<TextBox>,
    canvas: EditorCanvas,
    /// Where the canvas showed the picture last time.
    view: View,
    cursor: (f32, f32),
    modifiers: ModifiersState,
    saves: SaveTracker,
    proxy: EventLoopProxy<AppEvent>,
}

impl EditorWindow {
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        event_loop: &ActiveEventLoop,
        image: Arc<PixelBuffer>,
        path: PathBuf,
        settings: EditorSettings,
        texts: &'static Texts,
        fonts: Arc<Fonts>,
        work_area: Option<PixelRect>,
        scale: f64,
        proxy: EventLoopProxy<AppEvent>,
    ) -> Option<EditorWindow> {
        let (w, h) = initial_size(image.bounds.size(), work_area, scale);
        let mut attrs = Window::default_attributes()
            .with_title(title(&path))
            .with_window_icon(super::window_icon())
            .with_min_inner_size(LogicalSize::new(360.0, 240.0))
            .with_inner_size(LogicalSize::new(w, h));
        if let Some(a) = work_area {
            let (pw, ph) = ((w as f64 * scale) as i32, (h as f64 * scale) as i32);
            let x = a.x + (a.width - pw).max(0) / 2;
            let y = a.y + (a.height - ph).max(0) / 2;
            attrs = attrs.with_position(PhysicalPosition::new(x, y));
        }
        let window = event_loop.create_window(attrs).ok()?;
        let mut editor = EditorWindow {
            surface: WindowSurface::new(window),
            doc: Document::new(image),
            path,
            settings,
            texts,
            fonts,
            ui: Ui::new(Vec::new()),
            tool: Tool::Arrow,
            color: 0,
            size: Size::Medium,
            drag: None,
            text: None,
            canvas: EditorCanvas::default(),
            view: View::IDENTITY,
            cursor: (-1.0, -1.0),
            modifiers: ModifiersState::empty(),
            saves: SaveTracker::default(),
            proxy,
        };
        editor.rebuild();
        editor.focus();
        Some(editor)
    }

    pub fn id(&self) -> WindowId {
        self.surface.window.id()
    }

    pub fn focus(&self) {
        self.surface.window.set_minimized(false);
        self.surface.window.focus_window();
    }

    pub fn set_visible(&self, visible: bool) {
        self.surface.window.set_visible(visible);
    }

    fn scale(&self) -> f32 {
        self.surface.window.scale_factor() as f32
    }

    fn logical_size(&self) -> (f32, f32) {
        let s = self.surface.size();
        (s.width as f32 / self.scale(), s.height as f32 / self.scale())
    }

    fn bar_state(&self) -> BarState {
        BarState {
            tool: self.tool,
            color: self.color,
            size: self.size,
            can_undo: self.doc.can_undo(),
            can_redo: self.doc.can_redo(),
            saving: self.saves.busy(),
        }
    }

    fn rebuild(&mut self) {
        let mut ui = build_bar(self.logical_size().0, &self.bar_state(), self.texts, &self.fonts);
        ui.carry_state_from(&self.ui);
        self.ui = ui;
        self.surface.window.request_redraw();
    }

    fn canvas_area(&self) -> RectF {
        let top = self.ui.bounds().bottom() + PAD;
        let (w, h) = self.logical_size();
        RectF { x: 0.0, y: top, w, h: (h - top).max(1.0) }
    }

    fn in_canvas(&self) -> bool {
        self.cursor.1 >= self.canvas_area().y
    }

    fn text_annotation(&self) -> Option<Annotation> {
        let t = self.text.as_ref()?;
        Some(Annotation::Text {
            at: t.at,
            text: format!("{}{}", t.text, t.preedit),
            color: PALETTE[self.color],
            px: self.size.text_px(),
        })
    }

    fn redraw(&mut self) {
        let size = self.surface.size();
        if size.width <= 0 || size.height <= 0 {
            return;
        }
        let mut painter = Painter::new(size, self.scale());
        let preview = self.drag.as_ref().and_then(|d| match (d, self.tool) {
            (_, Tool::Crop | Tool::Pixelate) => None,
            (d, tool) => {
                match finish_drag(d, tool, self.color, self.size, self.doc.image_rect().size()) {
                    DragEnd::Add(a) => Some(a),
                    _ => None,
                }
            }
        });
        let selection = match (&self.drag, self.tool) {
            (Some(Drag::Shape { start, current }), Tool::Crop | Tool::Pixelate) => {
                let (a, b) = (self.view.to_canvas(*start), self.view.to_canvas(*current));
                Some(RectF {
                    x: a.0.min(b.0),
                    y: a.1.min(b.1),
                    w: (a.0 - b.0).abs(),
                    h: (a.1 - b.1).abs(),
                })
            }
            _ => None,
        };
        let text = self.text_annotation();
        let area = self.canvas_area();
        self.view = paint_editor(
            &mut painter,
            &self.ui,
            &mut self.canvas,
            &self.doc,
            preview.as_ref(),
            selection,
            text.as_ref().map(|a| (a, 12.0)),
            area,
            &self.fonts,
        );
        if let Some(Annotation::Text { at, text, px, .. }) = &text {
            // Keeps the input method's window next to the caret.
            let (x, y) = self.view.to_canvas(*at);
            let w = measure_text(&self.fonts, text, px * self.view.scale, true).0;
            let s = self.scale();
            self.surface.window.set_ime_cursor_area(
                PhysicalPosition::new(((x + w) * s) as i32, (y * s) as i32),
                PhysicalSize::new(2, (px * self.view.scale * s) as u32),
            );
        }
        let mut pixels = vec![0u32; (size.width * size.height) as usize];
        painter.to_softbuffer(&mut pixels);
        self.surface.present(&pixels);
    }

    fn image_point(&self) -> Point {
        self.view.to_image(self.cursor)
    }

    fn commit_text(&mut self) {
        let Some(t) = self.text.take() else { return };
        self.surface.window.set_ime_allowed(false);
        let text = format!("{}{}", t.text, t.preedit);
        if !text.trim().is_empty() {
            self.doc.add(Annotation::Text {
                at: t.at,
                text,
                color: PALETTE[self.color],
                px: self.size.text_px(),
            });
        }
        self.rebuild();
    }

    fn start_drag(&mut self) {
        self.commit_text();
        let p = self.image_point();
        match self.tool {
            Tool::Text => {
                self.text = Some(TextBox { at: p, text: String::new(), preedit: String::new() });
                self.surface.window.set_ime_allowed(true);
            }
            Tool::Pen | Tool::Highlighter => self.drag = Some(Drag::Stroke(vec![p])),
            _ => self.drag = Some(Drag::Shape { start: p, current: p }),
        }
        self.surface.window.request_redraw();
    }

    fn move_drag(&mut self) {
        let p = self.image_point();
        match &mut self.drag {
            Some(Drag::Shape { current, .. }) => *current = p,
            Some(Drag::Stroke(points)) => {
                let far = points.last().is_none_or(|q| (q.0 - p.0).hypot(q.1 - p.1) >= 0.75);
                if far {
                    points.push(p);
                }
            }
            None => return,
        }
        self.surface.window.request_redraw();
    }

    fn end_drag(&mut self) {
        let Some(drag) = self.drag.take() else { return };
        let image = self.doc.image_rect().size();
        match finish_drag(&drag, self.tool, self.color, self.size, image) {
            DragEnd::Add(a) => self.doc.add(a),
            DragEnd::Crop(a, b) => self.doc.crop_to(a, b),
            DragEnd::Nothing => {}
        }
        self.rebuild();
    }

    fn name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    fn start_save(&mut self, path: PathBuf, close_after: bool) {
        self.commit_text();
        if !self.saves.begin(path.clone(), close_after) {
            return;
        }
        let snapshot = self.doc.snapshot();
        let version = self.doc.version();
        let (fonts, settings, proxy, window) =
            (self.fonts.clone(), self.settings, self.proxy.clone(), self.id());
        std::thread::spawn(move || {
            let img = snapshot.flatten(&fonts);
            let result = gyozarec::editor::save(&img, &path, settings.jpeg_quality)
                .map_err(|e| e.to_string());
            if result.is_ok() && settings.copy_to_clipboard {
                Win32Clipboard.set_image(&img);
            }
            let _ = proxy.send_event(AppEvent::EditorSaved { window, version, result });
        });
        self.rebuild();
    }

    /// The save thread finished.
    pub fn saved(&mut self, version: u64, result: Result<(), String>) -> EditorResult {
        if !self.saves.busy() {
            return EditorResult::Open;
        }
        if result.is_ok() {
            self.doc.mark_saved_at(version);
        }
        let still_dirty = self.doc.is_dirty() || self.text.is_some();
        match self.saves.finish(result.is_ok(), still_dirty) {
            SaveEnd::Close => return EditorResult::Closed,
            SaveEnd::SaveAgain(path) => self.start_save(path, true),
            SaveEnd::Stay { adopt: Some(path) } if path != self.path => {
                self.surface.window.set_title(&title(&path));
                self.path = path;
            }
            SaveEnd::Stay { .. } => {}
        }
        if let Err(error) = result {
            show_error_box(&self.texts.editor_save_failed.replace("{error}", &error));
        }
        self.rebuild();
        EditorResult::Open
    }

    fn save_as(&mut self) {
        self.commit_text();
        if self.saves.busy() {
            return; // the window would name a file that is not written yet
        }
        let folder = self.path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let picked = rfd::FileDialog::new()
            .set_directory(folder)
            .set_file_name(self.name())
            .add_filter("PNG", &["png"])
            .add_filter("JPG", &["jpg", "jpeg"])
            .save_file();
        if let Some(path) = picked {
            // The window takes the new name once the file is written.
            self.start_save(path, false);
        }
    }

    fn copy(&mut self) {
        self.commit_text();
        let (snapshot, fonts) = (self.doc.snapshot(), self.fonts.clone());
        std::thread::spawn(move || {
            Win32Clipboard.set_image(&snapshot.flatten(&fonts));
        });
    }

    /// Asks about unsaved changes; `false` means stay open (Cancel, or the
    /// window closes once saving has finished).
    pub fn confirm_close(&mut self) -> bool {
        self.commit_text();
        if self.saves.busy() {
            self.saves.begin(self.path.clone(), true);
            return false;
        }
        if !self.doc.is_dirty() {
            return true;
        }
        let question = self.texts.editor_save_question.replace("{name}", &self.name());
        match ask_save_changes(self.surface.hwnd(), &question) {
            SaveChoice::Save => {
                self.start_save(self.path.clone(), true);
                false
            }
            SaveChoice::Discard => true,
            SaveChoice::Cancel => false,
        }
    }

    fn run(&mut self, command: EditorCommand) -> EditorResult {
        match command {
            EditorCommand::Undo => {
                self.commit_text();
                self.doc.undo();
            }
            EditorCommand::Redo => {
                self.commit_text();
                self.doc.redo();
            }
            EditorCommand::Save => self.start_save(self.path.clone(), false),
            EditorCommand::SaveAs => self.save_as(),
            EditorCommand::Copy => self.copy(),
            EditorCommand::Close => {
                if self.confirm_close() {
                    return EditorResult::Closed;
                }
            }
        }
        self.rebuild();
        EditorResult::Open
    }

    fn click(&mut self, id: u32) -> EditorResult {
        match id {
            ID_UNDO => return self.run(EditorCommand::Undo),
            ID_REDO => return self.run(EditorCommand::Redo),
            ID_COPY => return self.run(EditorCommand::Copy),
            ID_SAVE_AS => return self.run(EditorCommand::SaveAs),
            ID_SAVE => return self.run(EditorCommand::Save),
            ID_SIZE => self.show_size_menu(),
            id if (ID_TOOL..ID_TOOL + TOOLS.len() as u32).contains(&id) => {
                self.commit_text();
                self.tool = TOOLS[(id - ID_TOOL) as usize];
            }
            id if (ID_SWATCH..ID_SWATCH + PALETTE.len() as u32).contains(&id) => {
                self.color = (id - ID_SWATCH) as usize;
            }
            _ => {}
        }
        self.rebuild();
        EditorResult::Open
    }

    fn menu_id(&self, index: usize) -> String {
        format!("{MENU_PREFIX}{}:size:{index}", u64::from(self.id()))
    }

    fn show_size_menu(&self) {
        let menu = Menu::new();
        for (i, size) in SIZES.iter().enumerate() {
            let label = self.texts.editor_sizes[i];
            let item =
                CheckMenuItem::with_id(self.menu_id(i), label, true, *size == self.size, None);
            let _ = menu.append(&item);
        }
        let rect = self.ui.widget(ID_SIZE).map(|w| w.rect).unwrap_or_default();
        let s = self.scale();
        let at = muda::dpi::PhysicalPosition::new((rect.x * s) as i32, (rect.bottom() * s) as i32);
        // SAFETY: the HWND belongs to this live window on the UI thread.
        unsafe {
            menu.show_context_menu_for_hwnd(
                self.surface.hwnd(),
                Some(muda::dpi::Position::Physical(at)),
            );
        }
    }

    /// A choice from this editor's size menu; other editors' ids are ignored.
    pub fn on_menu(&mut self, menu_id: &str) {
        if let Some(i) = (0..SIZES.len()).find(|i| self.menu_id(*i) == menu_id) {
            self.size = SIZES[i];
            self.rebuild();
        }
    }

    fn type_key(&mut self, event: &KeyEvent) {
        let Some(t) = &mut self.text else { return };
        match &event.logical_key {
            Key::Named(NamedKey::Enter) => {
                self.commit_text();
                return;
            }
            Key::Named(NamedKey::Escape) => {
                self.text = None;
                self.surface.window.set_ime_allowed(false);
            }
            Key::Named(NamedKey::Backspace) => {
                t.text.pop();
            }
            _ if self.modifiers.control_key() || self.modifiers.alt_key() => return,
            _ => match &event.text {
                Some(typed) if t.preedit.is_empty() => {
                    t.text.extend(typed.chars().filter(|c| !c.is_control()));
                }
                _ => return,
            },
        }
        self.surface.window.request_redraw();
    }

    pub fn handle(&mut self, event: &WindowEvent) -> EditorResult {
        match event {
            WindowEvent::CloseRequested => {
                if self.confirm_close() {
                    return EditorResult::Closed;
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => self.rebuild(),
            WindowEvent::CursorMoved { position, .. } => {
                let s = self.scale();
                self.cursor = (position.x as f32 / s, position.y as f32 / s);
                if self.drag.is_some() {
                    self.move_drag();
                } else if self.ui.pointer_moved(self.cursor.0, self.cursor.1) {
                    self.surface.window.request_redraw();
                }
                let icon = match (self.in_canvas(), self.tool) {
                    (false, _) => CursorIcon::Default,
                    (true, Tool::Text) => CursorIcon::Text,
                    (true, _) => CursorIcon::Crosshair,
                };
                self.surface.window.set_cursor(icon);
            }
            WindowEvent::CursorLeft { .. } => {
                if self.ui.pointer_left() {
                    self.surface.window.request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                self.surface.window.request_redraw();
                match state {
                    ElementState::Pressed if self.in_canvas() => self.start_drag(),
                    ElementState::Pressed => self.ui.pointer_down(self.cursor.0, self.cursor.1),
                    ElementState::Released if self.drag.is_some() => self.end_drag(),
                    ElementState::Released => {
                        if let Some(id) = self.ui.pointer_up(self.cursor.0, self.cursor.1) {
                            return self.click(id);
                        }
                    }
                }
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                let typing = self.text.is_some();
                if let Some(command) = command_for(&event.logical_key, self.modifiers, typing) {
                    return self.run(command);
                }
                self.type_key(event);
            }
            WindowEvent::Ime(Ime::Preedit(text, _)) => {
                if let Some(t) = &mut self.text {
                    t.preedit = text.clone();
                    self.surface.window.request_redraw();
                }
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                if let Some(t) = &mut self.text {
                    t.preedit.clear();
                    t.text.push_str(text);
                    self.surface.window.request_redraw();
                }
            }
            _ => {}
        }
        EditorResult::Open
    }
}

fn title(path: &std::path::Path) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    format!("{name} — GyozaRec")
}

#[cfg(test)]
mod tests {
    use super::*;
    use gyozarec::i18n::Language;
    use winit::keyboard::SmolStr;

    fn key(c: &str) -> Key {
        Key::Character(SmolStr::new(c))
    }

    #[test]
    fn closing_during_a_save_with_newer_edits_saves_again() {
        let mut t = SaveTracker::default();
        assert!(t.begin(PathBuf::from("a.png"), false));
        assert!(!t.begin(PathBuf::from("a.png"), true), "joins the running save");
        assert_eq!(t.finish(true, true), SaveEnd::SaveAgain(PathBuf::from("a.png")));
        assert!(t.begin(PathBuf::from("a.png"), true));
        assert_eq!(t.finish(true, false), SaveEnd::Close);
        assert!(!t.busy());
    }

    #[test]
    fn a_failed_save_keeps_the_window_and_its_file() {
        let mut t = SaveTracker::default();
        t.begin(PathBuf::from("b.jpg"), true);
        assert_eq!(t.finish(false, true), SaveEnd::Stay { adopt: None });
    }

    #[test]
    fn save_as_is_refused_while_saving_and_adopted_only_once_written() {
        let mut t = SaveTracker::default();
        assert!(t.begin(PathBuf::from("a.png"), false));
        assert!(t.busy());
        assert!(!t.begin(PathBuf::from("elsewhere.png"), false));
        assert_eq!(t.finish(true, false), SaveEnd::Stay { adopt: Some(PathBuf::from("a.png")) });
        assert!(t.begin(PathBuf::from("elsewhere.png"), false));
        assert_eq!(
            t.finish(true, false),
            SaveEnd::Stay { adopt: Some(PathBuf::from("elsewhere.png")) }
        );
    }

    #[test]
    fn initial_size_fits_the_work_area() {
        let work = Some(PixelRect::new(0, 0, 1920, 1040));
        let (w, h) = initial_size(PixelSize { width: 7680, height: 2160 }, work, 1.0);
        assert!(w <= 1536.0 && h <= 832.0, "{w}×{h}");
        assert_eq!(initial_size(PixelSize { width: 12, height: 12 }, work, 1.0), (720.0, 420.0));
        let tiny_screen = Some(PixelRect::new(0, 0, 800, 400));
        let (w, h) = initial_size(PixelSize { width: 12, height: 12 }, tiny_screen, 1.0);
        assert!(w <= 720.0 && h <= 360.0, "fits a small screen: {w}×{h}");
        let (w, _) = initial_size(PixelSize { width: 1200, height: 600 }, work, 1.5);
        assert_eq!(w, (1200.0f32 / 1.5 + 24.0).round(), "one image pixel per screen pixel");
    }

    #[test]
    fn canvas_cache_survives_pointer_moves() {
        let fonts = Fonts::embedded();
        let base = Arc::new(PixelBuffer::solid(PixelRect::new(0, 0, 200, 100), 255, 255, 255));
        let mut doc = Document::new(base);
        let mut canvas = EditorCanvas::default();
        let area = RectF { x: 0.0, y: 0.0, w: 100.0, h: 50.0 };
        for _ in 0..100 {
            canvas.frame(&doc, area, 1.0, &fonts);
        }
        assert_eq!(canvas.rebuilds, 1);
        doc.add(Annotation::Rectangle {
            a: (1.0, 1.0),
            b: (50.0, 50.0),
            color: PALETTE[0],
            width: 3.0,
        });
        canvas.frame(&doc, area, 1.0, &fonts);
        assert_eq!(canvas.rebuilds, 2);
        let (img, _, dest) = canvas.frame(&doc, area, 1.0, &fonts);
        assert_eq!((img.width(), img.height()), (100, 50));
        assert_eq!(dest, area);
    }

    #[test]
    fn shortcuts_map_to_editor_commands() {
        let ctrl = ModifiersState::CONTROL;
        let ctrl_shift = ModifiersState::CONTROL | ModifiersState::SHIFT;
        assert_eq!(command_for(&key("z"), ctrl, false), Some(EditorCommand::Undo));
        assert_eq!(command_for(&key("Z"), ctrl_shift, false), Some(EditorCommand::Redo));
        assert_eq!(command_for(&key("y"), ctrl, false), Some(EditorCommand::Redo));
        assert_eq!(command_for(&key("s"), ctrl, true), Some(EditorCommand::Save));
        assert_eq!(command_for(&key("S"), ctrl_shift, false), Some(EditorCommand::SaveAs));
        assert_eq!(command_for(&key("c"), ctrl, false), Some(EditorCommand::Copy));
        assert_eq!(command_for(&key("c"), ctrl, true), None, "copy belongs to the text");
        assert_eq!(command_for(&key("s"), ModifiersState::empty(), true), None, "plain s is typed");
        let esc = Key::Named(NamedKey::Escape);
        assert_eq!(command_for(&esc, ModifiersState::empty(), false), Some(EditorCommand::Close));
        assert_eq!(command_for(&esc, ModifiersState::empty(), true), None);
    }

    #[test]
    fn redact_and_pixelate_follow_color_and_size() {
        let image = PixelSize { width: 1920, height: 1080 };
        let drag = Drag::Shape { start: (10.0, 10.0), current: (200.0, 80.0) };
        let redact = finish_drag(&drag, Tool::Redact, 4, Size::Thin, image);
        let black = Annotation::Redact { a: (10.0, 10.0), b: (200.0, 80.0), color: PALETTE[4] };
        assert_eq!(redact, DragEnd::Add(black));
        let blocks: Vec<_> = SIZES
            .iter()
            .map(|s| match finish_drag(&drag, Tool::Pixelate, 0, *s, image) {
                DragEnd::Add(Annotation::Pixelate { block, .. }) => block,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(blocks, [16.0, 32.0, 64.0]);
    }

    #[test]
    fn zero_length_drags_add_nothing() {
        let size = PixelSize { width: 100, height: 100 };
        let still = Drag::Shape { start: (10.0, 10.0), current: (10.0, 10.0) };
        for tool in [Tool::Arrow, Tool::Rectangle, Tool::Pixelate, Tool::Redact] {
            assert_eq!(finish_drag(&still, tool, 0, Size::Medium, size), DragEnd::Nothing);
        }
        let moved = Drag::Shape { start: (10.0, 10.0), current: (40.0, 30.0) };
        assert!(matches!(finish_drag(&moved, Tool::Arrow, 0, Size::Medium, size), DragEnd::Add(_)));
        assert!(matches!(
            finish_drag(&Drag::Stroke(vec![(1.0, 1.0)]), Tool::Pen, 0, Size::Thin, size),
            DragEnd::Add(_)
        ));
    }

    #[test]
    fn bar_wraps_when_narrow_in_every_language() {
        let fonts = Fonts::for_languages(&Language::ALL);
        let state = BarState {
            tool: Tool::Pen,
            color: 2,
            size: Size::Thick,
            can_undo: true,
            can_redo: false,
            saving: false,
        };
        for lang in Language::ALL {
            let wide = build_bar(1600.0, &state, lang.texts(), &fonts);
            let save = wide.widget(ID_SAVE).unwrap().rect;
            assert_eq!(save.y, PAD, "{lang:?} fits one row");
            assert!(save.right() <= 1600.0 - PAD + 0.01);
            let narrow = build_bar(500.0, &state, lang.texts(), &fonts);
            assert!(narrow.widget(ID_SAVE).unwrap().rect.y > PAD, "{lang:?} wraps");
            assert!(!narrow.widget(ID_REDO).unwrap().enabled);
        }
    }

    /// Draws the editor offscreen into `%TEMP%\gyozarec-editor-<lang>.png`.
    #[test]
    #[ignore = "desktop"]
    fn render_editor_preview() {
        let fonts = Fonts::for_languages(&Language::ALL);
        let mut base = PixelBuffer::solid(PixelRect::new(0, 0, 900, 500), 250, 245, 240);
        for y in 0..500 {
            for x in (0..900).step_by(60) {
                base.put_pixel(gyozarec::geometry::PixelPoint { x, y }, [200, 200, 200, 255]);
            }
        }
        let mut doc = Document::new(Arc::new(base));
        doc.add(Annotation::Arrow {
            from: (80.0, 400.0),
            to: (300.0, 200.0),
            color: PALETTE[0],
            width: 6.0,
        });
        doc.add(Annotation::Rectangle {
            a: (320.0, 120.0),
            b: (600.0, 260.0),
            color: PALETTE[3],
            width: 6.0,
        });
        doc.add(Annotation::Stroke {
            points: vec![(350.0, 330.0), (650.0, 330.0)],
            color: PALETTE[1],
            width: 6.0,
            highlight: true,
        });
        doc.add(Annotation::Text {
            at: (340.0, 300.0),
            text: "GyozaRec 日本語".into(),
            color: PALETTE[4],
            px: 28.0,
        });
        doc.add(Annotation::Pixelate { a: (650.0, 60.0), b: (850.0, 160.0), block: 10.0 });
        for lang in [Language::En, Language::Ja] {
            let state = BarState {
                tool: Tool::Arrow,
                color: 0,
                size: Size::Medium,
                can_undo: true,
                can_redo: false,
                saving: false,
            };
            let ui = build_bar(1000.0, &state, lang.texts(), &fonts);
            let top = ui.bounds().bottom() + PAD;
            let size = PixelSize { width: 1000, height: 640 };
            let mut p = Painter::new(size, 1.0);
            let area = RectF { x: 0.0, y: top, w: 1000.0, h: 640.0 - top };
            paint_editor(
                &mut p,
                &ui,
                &mut EditorCanvas::default(),
                &doc,
                None,
                None,
                None,
                area,
                &fonts,
            );
            let mut pixels = vec![0u32; 1000 * 640];
            p.to_softbuffer(&mut pixels);
            let img = image::RgbImage::from_fn(1000, 640, |x, y| {
                let c = pixels[(y * 1000 + x) as usize];
                image::Rgb([(c >> 16) as u8, (c >> 8) as u8, c as u8])
            });
            img.save(std::env::temp_dir().join(format!("gyozarec-editor-{}.png", lang.code())))
                .unwrap();
        }
    }
}
