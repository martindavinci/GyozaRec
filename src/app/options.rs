//! The Options window: save folder, toggles, recording parameters and language.

use super::surface::WindowSurface;
use gyozarec::i18n::{Language, Texts};
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
const ID_OPEN_FOLDER: u32 = 5;
const ID_LANGUAGE: u32 = 15;
const ID_OK: u32 = 20;
const ID_CANCEL: u32 = 21;
/// Menu id prefix for value choices: `opt:<combo id>:<value>`.
pub const MENU_PREFIX: &str = "opt:";
/// Menu id prefix for the language choice: `opt:lang:<code|auto>`.
const LANGUAGE_PREFIX: &str = "opt:lang:";

/// The numeric dropdowns: id, base choices.
const COMBOS: [(u32, &[u32]); 5] = [
    (10, &GIF_FPS_CHOICES),
    (11, &GIF_MAX_SECONDS_CHOICES),
    (12, &GIF_LONG_SIDE_CHOICES),
    (13, &VIDEO_FPS_CHOICES),
    (14, &VIDEO_MAX_SECONDS_CHOICES),
];

fn combo_label(id: u32, texts: &Texts) -> &'static str {
    match id {
        10 => texts.gif_fps,
        11 => texts.gif_max_seconds,
        12 => texts.gif_max_long_side,
        13 => texts.video_fps,
        _ => texts.video_max_seconds,
    }
}

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

fn language_label(language: Option<Language>, texts: &Texts) -> &'static str {
    language.map_or(texts.automatic, Language::native_name)
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
    texts: &'static Texts,
    ui_language: Language,
    /// Own fonts, so the native name of any chosen language can be drawn.
    fonts: Fonts,
    ui: Ui,
    logical: (f32, f32),
    cursor: (f32, f32),
}

fn label(text: &str) -> WidgetKind {
    WidgetKind::Label { text: text.into(), color: GYOZA.text }
}

/// Interface fonts plus whatever the chosen language's native name needs.
fn fonts_for(ui_language: Language, form: &OptionsForm) -> Fonts {
    Fonts::for_languages(&[ui_language, form.language.unwrap_or(ui_language)])
}

fn build_ui(form: &OptionsForm, texts: &Texts, fonts: &Fonts) -> Ui {
    let mut widgets: Vec<Widget> = Vec::new();
    let mut y = PAD;
    let push_row =
        |items: Vec<(u32, WidgetKind)>, x: f32, widgets: &mut Vec<Widget>, y: &mut f32| {
            let row = layout_row(items, x, *y, 10.0, fonts, &GYOZA);
            *y = row.iter().map(|w| w.rect.bottom()).fold(*y, f32::max) + ROW_GAP;
            widgets.extend(row);
        };

    push_row(vec![(100, label(texts.save_folder))], PAD, &mut widgets, &mut y);
    let path = shorten_middle(&form.save_folder.to_string_lossy(), PATH_CHARS);
    let browse = WidgetKind::Button { label: texts.browse.into(), dropdown: false };
    push_row(vec![(101, label(&path)), (ID_BROWSE, browse)], PAD, &mut widgets, &mut y);

    let checks = [
        (ID_CLIPBOARD, texts.copy_to_clipboard, form.copy_to_clipboard),
        (ID_SOUND, texts.play_sound, form.play_sound),
        (ID_CURSOR, texts.include_cursor, form.include_cursor),
        (ID_OPEN_FOLDER, texts.open_folder_after_recording, form.open_folder_after_recording),
    ];
    for (id, text, checked) in checks {
        let check = WidgetKind::Check { label: text.into(), checked };
        push_row(vec![(id, check)], PAD, &mut widgets, &mut y);
    }

    let dropdowns: Vec<(u32, &str, String)> = COMBOS
        .iter()
        .map(|(id, _)| (*id, combo_label(*id, texts), combo_value(form, *id).to_string()))
        .chain(std::iter::once((
            ID_LANGUAGE,
            texts.language,
            language_label(form.language, texts).to_string(),
        )))
        .collect();
    let label_w = dropdowns
        .iter()
        .map(|(_, text, _)| measure_text(fonts, text, GYOZA.font_px, false).0)
        .fold(0.0, f32::max);
    for (id, text, value) in dropdowns {
        let value = WidgetKind::Button { label: value, dropdown: true };
        let mut row = layout_row(vec![(200 + id, label(text))], PAD, y, 0.0, fonts, &GYOZA);
        let button = layout_row(vec![(id, value)], PAD + label_w + 16.0, y, 0.0, fonts, &GYOZA);
        let row_h = button[0].rect.h;
        row[0].rect.y = y + (row_h - row[0].rect.h) / 2.0;
        y += row_h + ROW_GAP;
        widgets.extend(row);
        widgets.extend(button);
    }

    y += ROW_GAP;
    let ok = WidgetKind::Button { label: texts.ok.into(), dropdown: false };
    let cancel = WidgetKind::Button { label: texts.cancel.into(), dropdown: false };
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
        ui_language: Language,
    ) -> Option<OptionsWindow> {
        let texts = ui_language.texts();
        let fonts = fonts_for(ui_language, &form);
        let ui = build_ui(&form, texts, &fonts);
        let logical = logical_size(&ui);
        let attrs = Window::default_attributes()
            .with_title(texts.options)
            .with_window_icon(super::window_icon())
            .with_resizable(false)
            .with_enabled_buttons(WindowButtons::CLOSE)
            .with_owner_window(owner)
            .with_inner_size(LogicalSize::new(logical.0, logical.1));
        let window = event_loop.create_window(attrs).ok()?;
        Some(OptionsWindow {
            surface: WindowSurface::new(window),
            form,
            texts,
            ui_language,
            fonts,
            ui,
            logical,
            cursor: (-1.0, -1.0),
        })
    }

    pub fn focus(&self) {
        self.surface.window.set_minimized(false);
        self.surface.window.focus_window();
    }

    pub fn id(&self) -> WindowId {
        self.surface.window.id()
    }

    fn scale(&self) -> f32 {
        self.surface.window.scale_factor() as f32
    }

    fn rebuild(&mut self) {
        let mut ui = build_ui(&self.form, self.texts, &self.fonts);
        ui.carry_state_from(&self.ui);
        self.ui = ui;
        let logical = logical_size(&self.ui);
        if logical != self.logical {
            self.logical = logical;
            let _ = self.surface.window.request_inner_size(LogicalSize::new(logical.0, logical.1));
        }
        self.surface.window.request_redraw();
    }

    fn redraw(&mut self) {
        let size = self.surface.size();
        if size.width <= 0 || size.height <= 0 {
            return;
        }
        let mut painter = Painter::new(size, self.scale());
        painter.clear(GYOZA.bg);
        self.ui.paint(&mut painter, &self.fonts, &GYOZA);
        let mut pixels = vec![0u32; (size.width * size.height) as usize];
        painter.to_softbuffer(&mut pixels);
        self.surface.present(&pixels);
    }

    /// Applies a choice from a value menu (`opt:<id>:<value>` or `opt:lang:<code|auto>`).
    pub fn on_menu(&mut self, menu_id: &str) {
        if let Some(code) = menu_id.strip_prefix(LANGUAGE_PREFIX) {
            self.form.language = Language::from_code(code);
            self.fonts = fonts_for(self.ui_language, &self.form);
            self.rebuild();
            return;
        }
        let mut parts = menu_id.trim_start_matches(MENU_PREFIX).split(':');
        let (Some(Ok(id)), Some(Ok(value))) =
            (parts.next().map(str::parse::<u32>), parts.next().map(str::parse::<u32>))
        else {
            return;
        };
        set_combo_value(&mut self.form, id, value);
        self.rebuild();
    }

    fn show_choices(&self, id: u32) {
        let menu = Menu::new();
        if id == ID_LANGUAGE {
            let choices = std::iter::once(None).chain(Language::ALL.iter().copied().map(Some));
            for choice in choices {
                let code = choice.map_or("auto", Language::code);
                let _ = menu.append(&CheckMenuItem::with_id(
                    format!("{LANGUAGE_PREFIX}{code}"),
                    language_label(choice, self.texts),
                    true,
                    choice == self.form.language,
                    None,
                ));
            }
        } else {
            let Some((_, base)) = COMBOS.iter().find(|(cid, _)| *cid == id) else { return };
            let current = combo_value(&self.form, id);
            for value in choices_with(base, current) {
                let _ = menu.append(&CheckMenuItem::with_id(
                    format!("{MENU_PREFIX}{id}:{value}"),
                    value.to_string(),
                    true,
                    value == current,
                    None,
                ));
            }
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

    fn browse(&mut self) {
        let picked = rfd::FileDialog::new()
            .set_title(self.texts.save_folder)
            .set_directory(&self.form.save_folder)
            .pick_folder();
        if let Some(folder) = picked {
            self.form.save_folder = folder;
            self.rebuild();
        }
    }

    fn click(&mut self, id: u32) -> OptionsResult {
        match id {
            ID_OK => return OptionsResult::Accepted(self.form.clone()),
            ID_CANCEL => return OptionsResult::Canceled,
            ID_BROWSE => self.browse(),
            ID_CLIPBOARD => self.form.copy_to_clipboard = !self.form.copy_to_clipboard,
            ID_SOUND => self.form.play_sound = !self.form.play_sound,
            ID_CURSOR => self.form.include_cursor = !self.form.include_cursor,
            ID_OPEN_FOLDER => {
                self.form.open_folder_after_recording = !self.form.open_folder_after_recording
            }
            id if id == ID_LANGUAGE || COMBOS.iter().any(|(cid, _)| *cid == id) => {
                self.show_choices(id)
            }
            _ => {}
        }
        self.rebuild();
        OptionsResult::Open
    }

    pub fn handle(&mut self, event: &WindowEvent) -> OptionsResult {
        match event {
            WindowEvent::CloseRequested => return OptionsResult::Canceled,
            WindowEvent::RedrawRequested => self.redraw(),
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
                    return self.click(id);
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
