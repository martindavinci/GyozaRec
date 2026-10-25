//! The Options window: save folder, toggles, recording parameters and language.

use super::surface::WindowSurface;
use gyozarec::i18n::{Language, Texts};
use gyozarec::presentation::{
    GIF_FPS_CHOICES, GIF_LONG_SIDE_CHOICES, GIF_MAX_SECONDS_CHOICES, JPEG_QUALITY_CHOICES,
    OptionsForm, VIDEO_FPS_CHOICES, VIDEO_MAX_SECONDS_CHOICES, choices_with, shorten_middle,
};
use gyozarec::uikit::painter::{Fonts, Painter, measure_text};
use gyozarec::uikit::theme::GYOZA;
use gyozarec::uikit::widgets::{Ui, Widget, WidgetKind, layout_row};
use muda::{CheckMenuItem, ContextMenu, Menu};
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, ModifiersState, NamedKey};
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
const ID_OPEN_FILE: u32 = 6;
const ID_TRAY: u32 = 7;
const ID_SHORTCUTS: u32 = 8;
const ID_PRINT_SCREEN: u32 = 9;
const ID_AUTOSTART: u32 = 30;
const ID_SYSTEM_AUDIO: u32 = 31;
/// Widest hint line before wrapping, in logical pixels.
const HINT_WIDTH: f32 = 340.0;
const COLUMN_GAP: f32 = 28.0;
const ID_LANGUAGE: u32 = 15;
const ID_OK: u32 = 20;
const ID_CANCEL: u32 = 21;
/// Menu id prefix for value choices: `opt:<combo id>:<value>`.
pub const MENU_PREFIX: &str = "opt:";
/// Menu id prefix for the language choice: `opt:lang:<code|auto>`.
const LANGUAGE_PREFIX: &str = "opt:lang:";

/// The numeric dropdowns: id, base choices.
const COMBOS: [(u32, &[u32]); 6] = [
    (16, &JPEG_QUALITY_CHOICES),
    (10, &GIF_FPS_CHOICES),
    (11, &GIF_MAX_SECONDS_CHOICES),
    (12, &GIF_LONG_SIDE_CHOICES),
    (13, &VIDEO_FPS_CHOICES),
    (14, &VIDEO_MAX_SECONDS_CHOICES),
];

fn combo_label(id: u32, texts: &Texts) -> &'static str {
    match id {
        16 => texts.jpeg_quality,
        10 => texts.gif_fps,
        11 => texts.gif_max_seconds,
        12 => texts.gif_max_long_side,
        13 => texts.video_fps,
        _ => texts.video_max_seconds,
    }
}

fn combo_value(form: &OptionsForm, id: u32) -> u32 {
    match id {
        16 => u32::from(form.jpeg_quality),
        10 => form.gif_fps,
        11 => form.gif_max_seconds,
        12 => form.gif_max_long_side,
        13 => form.video_fps,
        _ => form.video_max_seconds,
    }
}

fn set_combo_value(form: &mut OptionsForm, id: u32, value: u32) {
    match id {
        16 => form.jpeg_quality = value.clamp(50, 100) as u8,
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

/// One global shortcut as listed in Options.
#[derive(Clone, Debug)]
pub struct ShortcutLine {
    pub id: &'static str,
    pub label: &'static str,
    pub print_screen: bool,
    /// `false` when another app owns the keys.
    pub available: bool,
}

/// Editing state of the shortcut list.
#[derive(Default)]
struct ShortcutEdit {
    /// Line waiting for the user to press new keys.
    capturing: Option<&'static str>,
    /// Why the last keys pressed for a line were refused.
    errors: std::collections::HashMap<&'static str, &'static str>,
}

const ID_SHORTCUT_CHECK: u32 = 500;
const ID_SHORTCUT_KEYS: u32 = 520;
const ID_RESET_SHORTCUTS: u32 = 540;

/// The keys a line uses in the form being edited.
fn form_keys(form: &OptionsForm, id: &str) -> String {
    form.shortcut_keys
        .get(id)
        .cloned()
        .or_else(|| gyozarec::shortcuts::default_keys(id).map(str::to_string))
        .unwrap_or_default()
}

fn key_error_text(e: gyozarec::shortcuts::KeyError, texts: &Texts) -> &'static str {
    use gyozarec::shortcuts::KeyError;
    match e {
        KeyError::NoModifier => texts.shortcut_invalid,
        KeyError::Duplicate => texts.shortcut_duplicate,
        KeyError::Reserved | KeyError::Unparsable => texts.shortcut_unsupported,
    }
}

/// Greedy word wrap on spaces, so long translated hints stay readable.
fn wrap(text: &str, max: f32, fonts: &Fonts) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if !line.is_empty() && measure_text(fonts, &candidate, GYOZA.font_px, false).0 > max {
            lines.push(std::mem::replace(&mut line, word.to_string()));
        } else {
            line = candidate;
        }
    }
    lines.push(line);
    lines
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
    shortcuts: Vec<ShortcutLine>,
    edit: ShortcutEdit,
    modifiers: ModifiersState,
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

fn build_ui(
    form: &OptionsForm,
    texts: &Texts,
    shortcuts: &[ShortcutLine],
    edit: &ShortcutEdit,
    fonts: &Fonts,
) -> Ui {
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
        (ID_SYSTEM_AUDIO, texts.record_system_audio, form.record_system_audio),
        (ID_OPEN_FOLDER, texts.open_folder_after_recording, form.open_folder_after_recording),
        (ID_OPEN_FILE, texts.open_file_after_capture, form.open_file_after_capture),
        (ID_TRAY, texts.keep_in_tray, form.keep_in_tray),
        (ID_AUTOSTART, texts.start_with_windows, form.start_with_windows),
        (ID_SHORTCUTS, texts.shortcuts_enabled, form.shortcuts_enabled),
        (ID_PRINT_SCREEN, texts.print_screen_captures, form.print_screen_captures),
    ];
    for (id, text, checked) in checks {
        let check = WidgetKind::Check { label: text.into(), checked };
        push_row(vec![(id, check)], PAD, &mut widgets, &mut y);
    }
    // The Print Screen hint sits under its checkbox, in the left column.
    if form.print_screen_captures {
        for (i, line) in wrap(texts.print_screen_hint, HINT_WIDTH, fonts).into_iter().enumerate() {
            let kind = WidgetKind::Label { text: line, color: GYOZA.disabled_text };
            push_row(vec![(450 + i as u32, kind)], PAD + 26.0, &mut widgets, &mut y);
        }
    }
    let left_bottom = y;

    // Right column: recording parameters, language, then the shortcut list,
    // so the window stays short enough for small screens.
    let right_x = widgets.iter().map(|w| w.rect.right()).fold(0.0, f32::max) + COLUMN_GAP;
    let mut y = PAD;
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
        let mut row = layout_row(vec![(200 + id, label(text))], right_x, y, 0.0, fonts, &GYOZA);
        let button = layout_row(vec![(id, value)], right_x + label_w + 16.0, y, 0.0, fonts, &GYOZA);
        let row_h = button[0].rect.h;
        row[0].rect.y = y + (row_h - row[0].rect.h) / 2.0;
        y += row_h + ROW_GAP;
        widgets.extend(row);
        widgets.extend(button);
    }

    let listed: Vec<(usize, &ShortcutLine)> =
        shortcuts
            .iter()
            .enumerate()
            .filter(|(_, l)| {
                if l.print_screen { form.print_screen_captures } else { form.shortcuts_enabled }
            })
            .collect();
    if !listed.is_empty() {
        y += ROW_GAP;
        let mut title = vec![(400, label(texts.shortcuts_title))];
        if form.shortcuts_enabled {
            let reset = WidgetKind::Button { label: texts.reset_shortcuts.into(), dropdown: false };
            title.push((ID_RESET_SHORTCUTS, reset));
        }
        push_row(title, right_x, &mut widgets, &mut y);
        // Key buttons line up in a column after the longest label.
        let check_w = listed
            .iter()
            .map(|(_, l)| measure_text(fonts, l.label, GYOZA.font_px, false).0 + 22.0)
            .fold(0.0, f32::max);
        let keys_x = right_x + check_w + 12.0;
        for (i, l) in listed {
            let enabled = !form.shortcuts_disabled.contains(l.id);
            let check = WidgetKind::Check { label: l.label.into(), checked: enabled };
            let keys = if edit.capturing == Some(l.id) {
                texts.press_new_keys.to_string()
            } else {
                form_keys(form, l.id)
            };
            let mut items = vec![(
                ID_SHORTCUT_KEYS + i as u32,
                WidgetKind::Button { label: keys, dropdown: false },
            )];
            let note = edit
                .errors
                .get(l.id)
                .copied()
                .or_else(|| (!l.available && enabled).then_some(texts.shortcut_unavailable));
            if let Some(note) = note {
                let kind = WidgetKind::Label { text: note.into(), color: GYOZA.error };
                items.push((460 + i as u32, kind));
            }
            let right = layout_row(items, keys_x, y, 10.0, fonts, &GYOZA);
            let row_h = right[0].rect.h;
            let mut left = layout_row(
                vec![(ID_SHORTCUT_CHECK + i as u32, check)],
                right_x,
                y,
                0.0,
                fonts,
                &GYOZA,
            );
            left[0].rect.y = y + (row_h - left[0].rect.h) / 2.0;
            y += row_h + ROW_GAP;
            widgets.extend(left);
            widgets.extend(right);
        }
    }
    let mut y = y.max(left_bottom);

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
        shortcuts: Vec<ShortcutLine>,
    ) -> Option<OptionsWindow> {
        let texts = ui_language.texts();
        let fonts = fonts_for(ui_language, &form);
        let ui = build_ui(&form, texts, &shortcuts, &ShortcutEdit::default(), &fonts);
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
            shortcuts,
            edit: ShortcutEdit::default(),
            modifiers: ModifiersState::empty(),
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
        let mut ui = build_ui(&self.form, self.texts, &self.shortcuts, &self.edit, &self.fonts);
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

    /// Takes the keys pressed while a line waits for new ones.
    fn capture_keys(&mut self, event: &KeyEvent) {
        let Some(id) = self.edit.capturing else { return };
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {}
            Key::Named(NamedKey::Backspace) => {
                self.form.shortcut_keys.remove(id);
            }
            _ => {
                let Some(name) = super::hotkeys::key_name(&event.physical_key) else {
                    return; // a modifier alone: keep waiting
                };
                let m = self.modifiers;
                let mut parts: Vec<&str> = [
                    (m.control_key(), "Ctrl"),
                    (m.alt_key(), "Alt"),
                    (m.shift_key(), "Shift"),
                    (m.super_key(), "Super"),
                ]
                .iter()
                .filter(|(on, _)| *on)
                .map(|(_, name)| *name)
                .collect();
                parts.push(name);
                let others: Vec<(&str, String)> = self
                    .shortcuts
                    .iter()
                    .filter(|l| l.id != id)
                    .map(|l| (l.id, form_keys(&self.form, l.id)))
                    .collect();
                match gyozarec::shortcuts::validate(
                    &parts.join("+"),
                    &others,
                    super::hotkeys::parses,
                ) {
                    Ok(keys) => {
                        let default = gyozarec::shortcuts::default_keys(id)
                            .and_then(gyozarec::shortcuts::normalize);
                        if default.as_deref() == Some(keys.as_str()) {
                            self.form.shortcut_keys.remove(id);
                        } else {
                            self.form.shortcut_keys.insert(id.to_string(), keys);
                        }
                    }
                    Err(e) => {
                        self.edit.errors.insert(id, key_error_text(e, self.texts));
                    }
                }
            }
        }
        self.edit.capturing = None;
        self.rebuild();
    }

    fn click(&mut self, id: u32) -> OptionsResult {
        let line = |base: u32| {
            id.checked_sub(base)
                .filter(|i| (*i as usize) < self.shortcuts.len())
                .map(|i| i as usize)
        };
        if let Some(i) = line(ID_SHORTCUT_CHECK).filter(|_| id < ID_SHORTCUT_KEYS) {
            let sid = self.shortcuts[i].id.to_string();
            if !self.form.shortcuts_disabled.remove(&sid) {
                self.form.shortcuts_disabled.insert(sid);
            }
            self.rebuild();
            return OptionsResult::Open;
        }
        if let Some(i) = line(ID_SHORTCUT_KEYS).filter(|_| id < ID_RESET_SHORTCUTS) {
            let sid = self.shortcuts[i].id;
            self.edit.errors.remove(sid);
            self.edit.capturing = Some(sid);
            self.rebuild();
            return OptionsResult::Open;
        }
        if id == ID_RESET_SHORTCUTS {
            self.form.shortcut_keys.clear();
            self.form.shortcuts_disabled.clear();
            self.edit = ShortcutEdit::default();
            self.rebuild();
            return OptionsResult::Open;
        }
        match id {
            ID_OK => return OptionsResult::Accepted(self.form.clone()),
            ID_CANCEL => return OptionsResult::Canceled,
            ID_BROWSE => self.browse(),
            ID_CLIPBOARD => self.form.copy_to_clipboard = !self.form.copy_to_clipboard,
            ID_SOUND => self.form.play_sound = !self.form.play_sound,
            ID_CURSOR => self.form.include_cursor = !self.form.include_cursor,
            ID_SYSTEM_AUDIO => self.form.record_system_audio = !self.form.record_system_audio,
            ID_OPEN_FOLDER => {
                self.form.open_folder_after_recording = !self.form.open_folder_after_recording
            }
            ID_OPEN_FILE => self.form.open_file_after_capture = !self.form.open_file_after_capture,
            ID_TRAY => self.form.keep_in_tray = !self.form.keep_in_tray,
            ID_AUTOSTART => self.form.start_with_windows = !self.form.start_with_windows,
            ID_SHORTCUTS => self.form.shortcuts_enabled = !self.form.shortcuts_enabled,
            ID_PRINT_SCREEN => self.form.print_screen_captures = !self.form.print_screen_captures,
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
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && self.edit.capturing.is_some() =>
            {
                self.capture_keys(event);
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

#[cfg(test)]
mod layout_tests {
    use super::*;

    /// Fits a 768-pixel-high screen at 125% scaling (614 logical pixels
    /// minus the title bar), with every option that adds rows turned on.
    #[test]
    fn options_fit_a_small_screen_in_every_language() {
        let form = OptionsForm {
            shortcuts_enabled: true,
            print_screen_captures: true,
            ..OptionsForm::from_settings(&gyozarec::settings::AppSettings::default())
        };
        for lang in Language::ALL {
            let texts = lang.texts();
            let ids = ["image", "image-full", "record", "record-full", "print-screen"];
            let lines: Vec<ShortcutLine> = (0..5)
                .map(|i| ShortcutLine {
                    id: ids[i],
                    label: texts.quick_actions[i % 4],
                    print_screen: i == 4,
                    available: false,
                })
                .collect();
            let fonts = fonts_for(lang, &form);
            let edit = ShortcutEdit::default();
            let (w, h) = logical_size(&build_ui(&form, texts, &lines, &edit, &fonts));
            assert!(h <= 560.0, "{lang:?}: {h} logical pixels high");
            assert!(w <= 1000.0, "{lang:?}: {w} logical pixels wide");
        }
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    use gyozarec::uikit::painter::Painter;

    /// Draws Options offscreen into `%TEMP%\gyozarec-options.png` with the
    /// shortcut list open, one line waiting for keys and one refused.
    #[test]
    #[ignore = "desktop"]
    fn render_options_preview() {
        let mut form = OptionsForm {
            shortcuts_enabled: true,
            print_screen_captures: true,
            ..OptionsForm::from_settings(&gyozarec::settings::AppSettings::default())
        };
        form.shortcut_keys.insert("record".into(), "Ctrl+Alt+R".into());
        form.shortcuts_disabled.insert("image-full".into());
        let texts = Language::En.texts();
        let ids = ["image", "image-full", "record", "record-full", "print-screen"];
        let lines: Vec<ShortcutLine> = (0..5)
            .map(|i| ShortcutLine {
                id: ids[i],
                label: texts.quick_actions[i % 4],
                print_screen: i == 4,
                available: i != 3,
            })
            .collect();
        let mut edit = ShortcutEdit { capturing: Some("image"), ..Default::default() };
        edit.errors.insert("print-screen", texts.shortcut_duplicate);
        let fonts = fonts_for(Language::En, &form);
        let ui = build_ui(&form, texts, &lines, &edit, &fonts);
        let (w, h) = logical_size(&ui);
        let size = gyozarec::geometry::PixelSize { width: w as i32, height: h as i32 };
        let mut painter = Painter::new(size, 1.0);
        painter.clear(GYOZA.bg);
        ui.paint(&mut painter, &fonts, &GYOZA);
        let mut pixels = vec![0u32; (size.width * size.height) as usize];
        painter.to_softbuffer(&mut pixels);
        let img = image::RgbImage::from_fn(w as u32, h as u32, |x, y| {
            let c = pixels[(y * w as u32 + x) as usize];
            image::Rgb([(c >> 16) as u8, (c >> 8) as u8, c as u8])
        });
        img.save(std::env::temp_dir().join("gyozarec-options.png")).unwrap();
    }
}
