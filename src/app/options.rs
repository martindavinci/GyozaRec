//! The Options window: save folder, toggles and recording parameters.

use super::surface::WindowSurface;
use gyozarec::presentation::{
    GIF_FPS_CHOICES, GIF_LONG_SIDE_CHOICES, GIF_MAX_SECONDS_CHOICES, OptionsForm,
    VIDEO_FPS_CHOICES, VIDEO_MAX_SECONDS_CHOICES, choices_with, shorten_middle,
};
use gyozarec::uikit::painter::{Fonts, Painter, measure_text};
use gyozarec::uikit::theme::GYOZA;
use gyozarec::uikit::widgets::{Ui, Widget, WidgetKind, layout_row};
use muda::{CheckMenuItem, ContextMenu, Menu};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{Window, WindowButtons, WindowId};

const PAD: f32 = 12.0;
const ROW_GAP: f32 = 8.0;
const PATH_CHARS: usize = 42;
const ID_BROWSE: u32 = 1;
const ID_CLIPBOARD: u32 = 2;
const ID_SOUND: u32 = 3;
const ID_CURSOR: u32 = 4;
const ID_OK: u32 = 20;
const ID_CANCEL: u32 = 21;
/// Menu id prefix for value choices: `opt:<combo id>:<value>`.
pub const MENU_PREFIX: &str = "opt:";

/// The numeric dropdowns: id, label, base choices.
const COMBOS: [(u32, &str, &[u32]); 5] = [
    (10, "GIF: fotogrammi al secondo", &GIF_FPS_CHOICES),
    (11, "GIF: durata massima (s)", &GIF_MAX_SECONDS_CHOICES),
    (12, "GIF: lato lungo massimo (px)", &GIF_LONG_SIDE_CHOICES),
    (13, "Video: fotogrammi al secondo", &VIDEO_FPS_CHOICES),
    (14, "Video: durata massima (s)", &VIDEO_MAX_SECONDS_CHOICES),
];

fn combo_value(form: &OptionsForm, id: u32) -> u32 {
    match id {
        10 => form.gif_fps,
        11 => form.gif_max_seconds,
        12 => form.gif_max_long_side,
        13 => form.video_fps,
        _ => form.video_max_seconds,
    }
}

fn set_combo_value(form: &mut OptionsForm, id: u32, value: u32) {
    match id {
        10 => form.gif_fps = value,
        11 => form.gif_max_seconds = value,
        12 => form.gif_max_long_side = value,
        13 => form.video_fps = value,
        _ => form.video_max_seconds = value,
    }
}

/// What the app should do after an event in the Options window.
pub enum OptionsResult {
    Open,
    Accepted(OptionsForm),
    Canceled,
}

pub struct OptionsWindow {
    surface: WindowSurface,
    form: OptionsForm,
    ui: Ui,
    logical: (f32, f32),
    cursor: (f32, f32),
}

fn label(text: &str) -> WidgetKind {
    WidgetKind::Label { text: text.into(), color: GYOZA.text }
}

fn build_ui(form: &OptionsForm, fonts: &Fonts) -> Ui {
    let mut widgets: Vec<Widget> = Vec::new();
    let mut y = PAD;
    let push_row =
        |items: Vec<(u32, WidgetKind)>, x: f32, widgets: &mut Vec<Widget>, y: &mut f32| {
            let row = layout_row(items, x, *y, 10.0, fonts, &GYOZA);
            *y = row.iter().map(|w| w.rect.bottom()).fold(*y, f32::max) + ROW_GAP;
            widgets.extend(row);
        };

    push_row(vec![(100, label("Cartella di salvataggio"))], PAD, &mut widgets, &mut y);
    let path = shorten_middle(&form.save_folder.to_string_lossy(), PATH_CHARS);
    let browse = WidgetKind::Button { label: "Sfoglia…".into(), dropdown: false };
    push_row(vec![(101, label(&path)), (ID_BROWSE, browse)], PAD, &mut widgets, &mut y);

    let checks = [
        (ID_CLIPBOARD, "Copia negli appunti", form.copy_to_clipboard),
        (ID_SOUND, "Suono alla cattura", form.play_sound),
        (ID_CURSOR, "Includi cursore (video/GIF)", form.include_cursor),
    ];
    for (id, text, checked) in checks {
        let check = WidgetKind::Check { label: text.into(), checked };
        push_row(vec![(id, check)], PAD, &mut widgets, &mut y);
    }

    let label_w = COMBOS
        .iter()
        .map(|(_, text, _)| measure_text(fonts, text, GYOZA.font_px, false).0)
        .fold(0.0, f32::max);
    for (id, text, _) in COMBOS {
        let value = WidgetKind::Button { label: combo_value(form, id).to_string(), dropdown: true };
        let mut row = layout_row(vec![(200 + id, label(text))], PAD, y, 0.0, fonts, &GYOZA);
        let button = layout_row(vec![(id, value)], PAD + label_w + 16.0, y, 0.0, fonts, &GYOZA);
        let row_h = button[0].rect.h;
        row[0].rect.y = y + (row_h - row[0].rect.h) / 2.0;
        y += row_h + ROW_GAP;
        widgets.extend(row);
        widgets.extend(button);
    }

    y += ROW_GAP;
    let ok = WidgetKind::Button { label: "OK".into(), dropdown: false };
    let cancel = WidgetKind::Button { label: "Annulla".into(), dropdown: false };
    push_row(vec![(ID_OK, ok), (ID_CANCEL, cancel)], PAD, &mut widgets, &mut y);
    Ui::new(widgets)
}

fn logical_size(ui: &Ui) -> (f32, f32) {
    let b = ui.bounds();
    ((b.right() + PAD).ceil().max(380.0), (b.bottom() + PAD - ROW_GAP).ceil())
}

impl OptionsWindow {
    pub fn open(
        event_loop: &ActiveEventLoop,
        owner: isize,
        form: OptionsForm,
        fonts: &Fonts,
    ) -> Option<OptionsWindow> {
        let ui = build_ui(&form, fonts);
        let logical = logical_size(&ui);
        let attrs = Window::default_attributes()
            .with_title("Opzioni")
            .with_resizable(false)
            .with_enabled_buttons(WindowButtons::CLOSE)
            .with_owner_window(owner)
            .with_inner_size(LogicalSize::new(logical.0, logical.1));
        let window = event_loop.create_window(attrs).ok()?;
        Some(OptionsWindow {
            surface: WindowSurface::new(window),
            form,
            ui,
            logical,
            cursor: (-1.0, -1.0),
        })
    }

    pub fn id(&self) -> WindowId {
        self.surface.window.id()
    }

    fn scale(&self) -> f32 {
        self.surface.window.scale_factor() as f32
    }

    fn rebuild(&mut self, fonts: &Fonts) {
        self.ui = build_ui(&self.form, fonts);
        let logical = logical_size(&self.ui);
        if logical != self.logical {
            self.logical = logical;
            let _ = self.surface.window.request_inner_size(LogicalSize::new(logical.0, logical.1));
        }
        self.surface.window.request_redraw();
    }

    fn redraw(&mut self, fonts: &Fonts) {
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

    /// Applies a choice from a value menu (`opt:<id>:<value>`).
    pub fn on_menu(&mut self, menu_id: &str, fonts: &Fonts) {
        let mut parts = menu_id.trim_start_matches(MENU_PREFIX).split(':');
        let (Some(Ok(id)), Some(Ok(value))) =
            (parts.next().map(str::parse::<u32>), parts.next().map(str::parse::<u32>))
        else {
            return;
        };
        set_combo_value(&mut self.form, id, value);
        self.rebuild(fonts);
    }

    fn show_choices(&self, id: u32) {
        let Some((_, _, base)) = COMBOS.iter().find(|(cid, _, _)| *cid == id) else { return };
        let current = combo_value(&self.form, id);
        let menu = Menu::new();
        for value in choices_with(base, current) {
            let item_id = format!("{MENU_PREFIX}{id}:{value}");
            let _ = menu.append(&CheckMenuItem::with_id(
                item_id,
                value.to_string(),
                true,
                value == current,
                None,
            ));
        }
        let scale = self.scale();
        let rect = self.ui.widget(id).map(|w| w.rect).unwrap_or_default();
        let position = muda::dpi::Position::Physical(muda::dpi::PhysicalPosition::new(
            (rect.x * scale) as i32,
            (rect.bottom() * scale) as i32,
        ));
        // SAFETY: the HWND belongs to this live window on the UI thread.
        unsafe {
            menu.show_context_menu_for_hwnd(self.surface.hwnd(), Some(position));
        }
    }

    fn browse(&mut self, fonts: &Fonts) {
        let picked = rfd::FileDialog::new()
            .set_title("Cartella di salvataggio")
            .set_directory(&self.form.save_folder)
            .pick_folder();
        if let Some(folder) = picked {
            self.form.save_folder = folder;
            self.rebuild(fonts);
        }
    }

    fn click(&mut self, id: u32, fonts: &Fonts) -> OptionsResult {
        match id {
            ID_OK => return OptionsResult::Accepted(self.form.clone()),
            ID_CANCEL => return OptionsResult::Canceled,
            ID_BROWSE => self.browse(fonts),
            ID_CLIPBOARD => self.form.copy_to_clipboard = !self.form.copy_to_clipboard,
            ID_SOUND => self.form.play_sound = !self.form.play_sound,
            ID_CURSOR => self.form.include_cursor = !self.form.include_cursor,
            id if COMBOS.iter().any(|(cid, _, _)| *cid == id) => self.show_choices(id),
            _ => {}
        }
        self.rebuild(fonts);
        OptionsResult::Open
    }

    pub fn handle(&mut self, event: &WindowEvent, fonts: &Fonts) -> OptionsResult {
        match event {
            WindowEvent::CloseRequested => return OptionsResult::Canceled,
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
                } else if let Some(id) = self.ui.pointer_up(self.cursor.0, self.cursor.1) {
                    return self.click(id, fonts);
                }
            }
            WindowEvent::KeyboardInput { event: KeyEvent { logical_key, state, .. }, .. }
                if *state == ElementState::Pressed =>
            {
                match logical_key {
                    Key::Named(NamedKey::Escape) => return OptionsResult::Canceled,
                    Key::Named(NamedKey::Enter) => {
                        return OptionsResult::Accepted(self.form.clone());
                    }
                    _ => {}
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let s = *scale_factor as f32;
                let _ = self.surface.window.request_inner_size(winit::dpi::PhysicalSize::new(
                    (self.logical.0 * s).ceil() as u32,
                    (self.logical.1 * s).ceil() as u32,
                ));
                self.surface.window.request_redraw();
            }
            _ => {}
        }
        OptionsResult::Open
    }
}
