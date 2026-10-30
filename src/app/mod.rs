mod editor;
mod hotkeys;
mod notice;
mod options;
mod overlay;
#[cfg(test)]
mod readme_shot;
mod recording;
mod surface;
mod toolbar;
mod tray;

use gyozarec::bridge::{
    BridgeEditor, BridgeNotifier, BridgeRecordingUi, BridgeSelectionUi, BridgeToolbar,
    CaptureThread, UiRequest, UiSender,
};
use gyozarec::capture::{CaptureCoordinator, CursorLocator, ScreenGrabber};
use gyozarec::clock::{Clock, SystemClock};
use gyozarec::convert::{ConvertEnd, ConvertJob, convert_to_gif, gif_path_for};
use gyozarec::i18n::{Language, resolve};
use gyozarec::instance::InstanceCommand;
use gyozarec::paths::AppPaths;
use gyozarec::platform::{
    DefaultSinkFactory, GdiGrabber, SystemSound, Win32Clipboard, Win32Cursor, Win32Monitors,
    Win32WindowList, system_ui_languages,
};
use gyozarec::presentation::{
    AREAS, Activation, DELAYS, FORMATS, OptionsForm, ToolbarState, activation, area_label,
    delay_label, format_label,
};
use gyozarec::presentation::{StartupOptions, startup_visible};
use gyozarec::quick::{QuickAction, apply, can_start};
use gyozarec::settings::SettingsStore;
use gyozarec::shortcuts::{
    ClosePolicy, ExitPlan, Incoming, PRINT_SCREEN, SHORTCUTS, Shortcuts, accepts, close_policy,
    effective, exit_plan, rollback, stage,
};
use gyozarec::uikit::painter::Fonts;
use hotkeys::HotkeyRegistrar;
use muda::{CheckMenuItem, ContextMenu, Menu, MenuEvent};
use options::{OptionsResult, OptionsWindow, ShortcutLine};
use overlay::OverlaySession;
use recording::RecordingSession;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use toolbar::{Toolbar, ToolbarAction};
use tray::TrayCommand;
use tray_icon::TrayIcon;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::WindowId;

/// The gyoza icon for title bars and the taskbar (derived from GyozaRec.ico).
/// What title bars follow: 0 is Windows' own app mode, 1 dark.
static TITLE_THEME: AtomicU8 = AtomicU8::new(0);

/// The title bar look for new and restyled windows; `None` follows Windows,
/// which also tells the windows when its mode changes.
pub fn title_theme() -> Option<winit::window::Theme> {
    (TITLE_THEME.load(Ordering::Relaxed) == 1).then_some(winit::window::Theme::Dark)
}

pub fn window_icon() -> Option<winit::window::Icon> {
    let png = include_bytes!("../../assets/icon/gyozarec-256.png");
    let img = image::load_from_memory(png).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    winit::window::Icon::from_rgba(img.into_raw(), w, h).ok()
}

/// Format menu item that converts a video to GIF.
const CONVERT_MENU_ID: &str = "convert";

/// Events delivered to the UI thread through the event loop proxy.
pub enum AppEvent {
    Ui(UiRequest),
    Menu(MenuEvent),
    Instance(InstanceCommand),
    /// A registered global shortcut was pressed (its hotkey id).
    Shortcut(u32),
    /// Left click on the notification icon.
    TrayClick,
    Convert(ConvertEvent),
    /// The answer of the update check: a newer version and its page.
    #[cfg(feature = "github-updates")]
    UpdateChecked(Option<(String, String)>),
    /// The notification of that generation may close now.
    NoticeTimeout(u64),
    /// An editor's save thread finished the picture of `version`.
    EditorSaved {
        window: WindowId,
        version: u64,
        result: Result<(), String>,
    },
}

/// News from the video conversion thread.
pub enum ConvertEvent {
    Progress(u8),
    Done(PathBuf),
    Failed(gyozarec::Failure),
    Canceled,
}

/// A video being turned into a GIF on its own thread.
struct Conversion {
    cancel: Arc<AtomicBool>,
    handle: std::thread::JoinHandle<()>,
}

/// Sends coordinator requests into the event loop.
#[derive(Clone)]
pub struct ProxySender(pub EventLoopProxy<AppEvent>);

impl UiSender for ProxySender {
    fn send(&self, req: UiRequest) -> bool {
        self.0.send_event(AppEvent::Ui(req)).is_ok()
    }
}

pub struct App {
    fonts: Fonts,
    store: SettingsStore,
    state: ToolbarState,
    capture: CaptureThread,
    toolbar: Option<Toolbar>,
    overlay: Option<OverlaySession>,
    recording: Option<RecordingSession>,
    options: Option<OptionsWindow>,
    capture_dir: PathBuf,
    /// Command-line capture to run as soon as the toolbar exists.
    action_on_start: Option<QuickAction>,
    /// Launched at sign-in.
    autostart: bool,
    tray: Option<TrayIcon>,
    /// Language of the tray menu, to rebuild it when the language changes.
    tray_language: Option<Language>,
    hotkeys: HotkeyRegistrar,
    shortcuts: Shortcuts,
    /// The user closed the toolbar to the tray; captures must not bring it back.
    toolbar_hidden: bool,
    /// Quit as soon as the running capture has ended.
    exit_pending: bool,
    proxy: EventLoopProxy<AppEvent>,
    conversion: Option<Conversion>,
    editors: Vec<editor::EditorWindow>,
    /// Fonts for typed annotations, with every script; loaded on first use.
    editor_fonts: Option<Arc<Fonts>>,
    /// Windows' app mode, for the automatic theme.
    system_theme: Option<winit::window::Theme>,
    /// An editor opened during the running capture gets the focus after it.
    focus_editor: bool,
    notice: Option<notice::NoticeWindow>,
    /// The editor whose New started the running capture; it gives way to
    /// the new editor if it has nothing unsaved.
    replace_editor: Option<WindowId>,
    notice_timer: gyozarec::presentation::NoticeTimer,
}

impl App {
    pub fn new(proxy: EventLoopProxy<AppEvent>, startup: StartupOptions) -> App {
        let paths = AppPaths::current();
        let capture_dir = paths.capture_dir.clone();
        let store = SettingsStore::from_paths(&paths);
        let settings = store.load();
        let language = resolve(settings.language(), &system_ui_languages());
        let state = ToolbarState::new(settings, language);
        let sender = ProxySender(proxy.clone());
        let grabber: Arc<dyn ScreenGrabber> = Arc::new(GdiGrabber);
        let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
        let coordinator = CaptureCoordinator {
            monitors: Box::new(Win32Monitors),
            grabber: grabber.clone(),
            cursor: Box::new(Win32Cursor),
            windows: Box::new(Win32WindowList { exclude_pid: std::process::id() }),
            selection: Box::new(BridgeSelectionUi { sender: sender.clone() }),
            clipboard: Box::new(Win32Clipboard),
            sound: Box::new(SystemSound),
            revealer: Box::new(gyozarec::platform::ExplorerRevealer),
            editor: Box::new(BridgeEditor { sender: sender.clone() }),
            notifier: Box::new(BridgeNotifier { sender: sender.clone() }),
            toolbar: Box::new(BridgeToolbar { sender: sender.clone() }),
            recording_ui: Box::new(BridgeRecordingUi {
                sender: sender.clone(),
                grabber,
                clock: clock.clone(),
                sound: Box::new(SystemSound),
            }),
            sinks: Box::new(DefaultSinkFactory),
            audio: Box::new(gyozarec::platform::LoopbackStarter),
            clock,
        };
        let capture = CaptureThread::spawn(coordinator, sender);
        let menu_proxy = Mutex::new(proxy.clone());
        MenuEvent::set_event_handler(Some(move |event| {
            let _ = menu_proxy.lock().unwrap().send_event(AppEvent::Menu(event));
        }));
        let hotkey_proxy = Mutex::new(proxy.clone());
        global_hotkey::GlobalHotKeyEvent::set_event_handler(Some(
            move |event: global_hotkey::GlobalHotKeyEvent| {
                // Releases would start a second capture after a quick one.
                if event.state == global_hotkey::HotKeyState::Pressed {
                    let _ = hotkey_proxy.lock().unwrap().send_event(AppEvent::Shortcut(event.id));
                }
            },
        ));
        let tray_proxy = Mutex::new(proxy.clone());
        tray_icon::TrayIconEvent::set_event_handler(Some(move |event| {
            if let tray_icon::TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                button_state: tray_icon::MouseButtonState::Up,
                ..
            } = event
            {
                let _ = tray_proxy.lock().unwrap().send_event(AppEvent::TrayClick);
            }
        }));
        App {
            fonts: Fonts::for_language(language),
            store,
            state,
            capture,
            toolbar: None,
            overlay: None,
            recording: None,
            options: None,
            capture_dir,
            action_on_start: startup.action,
            autostart: startup.autostart,
            tray: None,
            tray_language: None,
            hotkeys: HotkeyRegistrar::new(),
            shortcuts: Shortcuts::default(),
            toolbar_hidden: false,
            exit_pending: false,
            proxy,
            conversion: None,
            editors: Vec::new(),
            editor_fonts: None,
            system_theme: None,
            focus_editor: false,
            notice: None,
            replace_editor: None,
            notice_timer: Default::default(),
        }
    }

    /// Creates, rebuilds or removes the notification icon to match the settings.
    fn sync_tray(&mut self) {
        let want = self.state.settings.keep_in_tray;
        if want && (self.tray.is_none() || self.tray_language != Some(self.state.language)) {
            self.tray = None;
            self.tray = tray::create(self.state.texts());
            self.tray_language = self.tray.as_ref().map(|_| self.state.language);
        } else if !want && self.tray.is_some() {
            self.tray = None;
            self.tray_language = None;
        }
        if self.tray.is_none() && self.toolbar_hidden {
            // The tray was the only way back to a closed toolbar.
            self.show_toolbar();
        }
    }

    /// Keeps the sign-in entry in line with the setting and the current path.
    fn sync_autostart(&self) {
        if let Ok(exe) = std::env::current_exe() {
            let store = gyozarec::platform::RunKeyStore { value_name: "GyozaRec" };
            gyozarec::autostart::sync(&store, self.state.settings.start_with_windows, &exe);
        }
    }

    fn sync_shortcuts(&mut self) {
        self.shortcuts.apply(effective(&self.state.settings), &mut self.hotkeys);
    }

    /// Shortcuts and their availability, for Options.
    fn shortcut_lines(&self) -> Vec<ShortcutLine> {
        let texts = self.state.texts();
        let available =
            |id: &str| self.shortcuts.status(id) != Some(Err(gyozarec::shortcuts::Unavailable));
        let mut lines: Vec<ShortcutLine> = SHORTCUTS
            .iter()
            .zip(texts.quick_actions)
            .map(|(b, label)| ShortcutLine {
                id: b.id,
                label,
                print_screen: false,
                available: available(b.id),
            })
            .collect();
        lines.push(ShortcutLine {
            id: PRINT_SCREEN.id,
            label: texts.quick_actions[0],
            print_screen: true,
            available: available(PRINT_SCREEN.id),
        });
        lines
    }

    fn show_toolbar(&mut self) {
        self.toolbar_hidden = false;
        if let Some(t) = &self.toolbar {
            t.surface.window.set_minimized(false);
            t.surface.window.set_visible(true);
            t.surface.window.focus_window();
        }
    }

    /// Asks for a video and turns it into a GIF next to it, with the GIF options.
    fn start_conversion(&mut self) {
        if !self.state.can_convert() || self.options.is_some() || self.exit_pending {
            return;
        }
        let texts = self.state.texts();
        let Some(source) = rfd::FileDialog::new()
            .set_title(texts.convert_video)
            .add_filter("MP4", &["mp4"])
            .pick_file()
        else {
            return;
        };
        let s = &self.state.settings;
        let job = ConvertJob {
            target: gif_path_for(&source),
            source,
            fps: s.gif_fps,
            max_long_side: s.gif_max_long_side,
            max_duration: std::time::Duration::from_secs(u64::from(s.gif_max_seconds)),
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let (flag, proxy) = (cancel.clone(), self.proxy.clone());
        let send = move |event| {
            let _ = proxy.send_event(AppEvent::Convert(event));
        };
        let spawned =
            std::thread::Builder::new().name("gyozarec-convert".into()).spawn(move || {
                let mut shown = None;
                let end =
                    convert_to_gif(&gyozarec::platform::MfVideoReader, &job, &flag, &mut |p| {
                        let percent = (p * 100.0) as u8;
                        if shown.replace(percent) != Some(percent) {
                            send(ConvertEvent::Progress(percent));
                        }
                    });
                send(match end {
                    Ok(ConvertEnd::Done) => ConvertEvent::Done(job.target),
                    Ok(ConvertEnd::Canceled) => ConvertEvent::Canceled,
                    Err(err) => ConvertEvent::Failed(gyozarec::Failure::from(&err)),
                });
            });
        if let Ok(handle) = spawned {
            self.conversion = Some(Conversion { cancel, handle });
            self.state.conversion_progress(0);
            self.refresh_toolbar();
        }
    }

    fn on_conversion(&mut self, event: ConvertEvent) {
        match event {
            ConvertEvent::Progress(percent) => self.state.conversion_progress(percent),
            end => {
                if let Some(c) = self.conversion.take() {
                    let _ = c.handle.join();
                }
                match end {
                    ConvertEvent::Done(path) => self.state.conversion_done(path),
                    ConvertEvent::Failed(failure) => self.state.conversion_failed(&failure),
                    _ => self.state.conversion_canceled(),
                }
            }
        }
        self.refresh_toolbar();
    }

    /// Opens an editor on an image, saved at `path` unless `on_disk` is false.
    #[allow(clippy::too_many_arguments)]
    fn open_editor(
        &mut self,
        event_loop: &ActiveEventLoop,
        image: Arc<gyozarec::pixels::PixelBuffer>,
        path: PathBuf,
        on_disk: bool,
        work_area: Option<gyozarec::geometry::PixelRect>,
        scale: f64,
    ) {
        let fonts = self
            .editor_fonts
            .get_or_insert_with(|| Arc::new(Fonts::for_languages(&Language::ALL)))
            .clone();
        let s = &self.state.settings;
        let settings = editor::EditorSettings {
            // Without a saved file the clipboard is where the picture goes.
            copy_to_clipboard: s.copy_to_clipboard || s.edit_before_saving,
            jpeg_quality: s.jpeg_quality,
            ask_to_save: s.editor_asks_to_save,
        };
        let opened = editor::EditorWindow::open(
            event_loop,
            image,
            path,
            on_disk,
            settings,
            self.state.texts(),
            fonts,
            work_area,
            scale,
            self.proxy.clone(),
        );
        self.editors.extend(opened);
    }

    /// Asks GitHub for a newer release, at most once a day.
    #[cfg(feature = "github-updates")]
    fn check_for_updates(&mut self) {
        use gyozarec::update;
        let now = chrono::Utc::now().timestamp();
        let s = &mut self.state.settings;
        if !s.check_for_updates || !update::due(s.last_update_check, now) {
            return;
        }
        // Saved first: quitting before the answer must not mean a request at every start.
        s.last_update_check = Some(now);
        self.save_settings();
        let proxy = self.proxy.clone();
        let _ = std::thread::Builder::new().name("gyozarec-update".into()).spawn(move || {
            let found =
                update::check(&gyozarec::platform::GitHubReleases, update::Version::current());
            let note = found.map(|r| (r.version.to_string(), r.page));
            let _ = proxy.send_event(AppEvent::UpdateChecked(note));
        });
    }

    /// Closes the notification `generation` after `NOTICE_TIMEOUT`.
    fn schedule_notice_timeout(&self, generation: u64) {
        let proxy = self.proxy.clone();
        let _ = std::thread::Builder::new().name("gyozarec-notice".into()).spawn(move || {
            std::thread::sleep(gyozarec::presentation::NOTICE_TIMEOUT);
            let _ = proxy.send_event(AppEvent::NoticeTimeout(generation));
        });
    }

    /// The notification was clicked: images open in the editor, videos in
    /// their default app.
    fn open_notice(&mut self, event_loop: &ActiveEventLoop, n: gyozarec::presentation::Notice) {
        match n.kind {
            gyozarec::presentation::NoticeKind::Image => match gyozarec::editor::load(&n.path) {
                Ok(img) => {
                    let area = Some(n.work_area);
                    self.open_editor(event_loop, Arc::new(img), n.path, true, area, n.scale)
                }
                Err(err) => {
                    self.state.set_error(&err.to_string());
                    self.refresh_toolbar();
                }
            },
            gyozarec::presentation::NoticeKind::Video => {
                use gyozarec::capture::FileRevealer;
                gyozarec::platform::ExplorerRevealer.open(&n.path);
            }
        }
    }

    /// Cancels a running conversion and waits for it; its partial GIF is deleted.
    fn stop_conversion(&mut self) {
        if let Some(c) = self.conversion.take() {
            c.cancel.store(true, Ordering::SeqCst);
            let _ = c.handle.join();
        }
    }

    /// Quits now, or as soon as the running capture has been cancelled.
    fn request_exit(&mut self, event_loop: &ActiveEventLoop) {
        // Unsaved edits are asked about first; Cancel keeps GyozaRec running.
        for e in &mut self.editors {
            if !e.confirm_close() {
                return;
            }
        }
        self.stop_conversion();
        let selecting = self.overlay.as_ref().is_some_and(|o| !o.is_finished());
        let stage = stage(self.state.busy, selecting, self.recording.is_some());
        match exit_plan(stage) {
            ExitPlan::ExitNow => event_loop.exit(),
            ExitPlan::WaitForCapture => self.exit_pending = true,
            ExitPlan::CancelSelection => {
                self.exit_pending = true;
                if let Some(mut overlay) = self.overlay.take() {
                    overlay.cancel();
                }
            }
            ExitPlan::CancelRecording => {
                self.exit_pending = true;
                if let Some(recording) = &self.recording {
                    recording.cancel();
                }
            }
        }
    }

    fn refresh_toolbar(&mut self) {
        if let Some(t) = &mut self.toolbar {
            t.rebuild(&self.state, &self.fonts);
        }
    }

    fn save_settings(&mut self) {
        if let Err(err) = self.store.save(&self.state.settings) {
            self.state.set_error(&err.to_string());
        }
    }

    /// Starts a capture unless one is running or Options is open; quick
    /// actions override area and format for this capture only.
    fn start_quick(&mut self, action: QuickAction) {
        if !can_start(self.state.busy, self.options.is_some()) {
            return;
        }
        self.state.busy = true;
        if !self.capture.request(apply(action, &self.state.settings)) {
            self.state.busy = false;
            self.state.set_error(self.state.texts().err_capture_thread);
        }
        self.refresh_toolbar();
    }

    fn show_menu(&self, action: ToolbarAction) {
        let Some(t) = &self.toolbar else { return };
        let texts = self.state.texts();
        let s = &self.state.settings;
        // (menu id, label, checked)
        let items: Vec<(String, &str, bool)> = match action {
            ToolbarAction::AreaMenu => AREAS
                .iter()
                .enumerate()
                .filter(|(_, a)| a.offered_for(s.format))
                .map(|(i, a)| {
                    let checked = *a == s.area.for_format(s.format);
                    (format!("area:{i}"), area_label(*a, texts), checked)
                })
                .collect(),
            ToolbarAction::FormatMenu => FORMATS
                .iter()
                .enumerate()
                .map(|(i, f)| (format!("format:{i}"), format_label(*f, texts), *f == s.format))
                .collect(),
            _ => DELAYS
                .iter()
                .map(|d| (format!("delay:{d}"), delay_label(*d, texts), *d == s.delay_seconds))
                .collect(),
        };
        let menu = Menu::new();
        for (id, label, checked) in items {
            let _ = menu.append(&CheckMenuItem::with_id(id, label, true, checked, None));
        }
        if action == ToolbarAction::FormatMenu {
            let _ = menu.append(&muda::PredefinedMenuItem::separator());
            let convert = muda::MenuItem::with_id(
                CONVERT_MENU_ID,
                texts.convert_video,
                self.state.can_convert(),
                None,
            );
            let _ = menu.append(&convert);
        }
        let anchor = t.menu_anchor(action);
        let position =
            muda::dpi::Position::Physical(muda::dpi::PhysicalPosition::new(anchor.x, anchor.y));
        // SAFETY: the HWND belongs to the live toolbar window on this thread.
        unsafe {
            menu.show_context_menu_for_hwnd(t.surface.hwnd(), Some(position));
        }
    }

    fn open_options(&mut self, event_loop: &ActiveEventLoop) {
        if self.options.is_some() || self.state.busy {
            return;
        }
        let Some(t) = &self.toolbar else { return };
        let form = OptionsForm::from_settings(&self.state.settings);
        let lines = self.shortcut_lines();
        self.options = OptionsWindow::open(
            event_loop,
            t.surface.hwnd(),
            form,
            self.state.language,
            lines,
            self.state.update.clone(),
        );
    }

    fn close_options(&mut self, result: OptionsResult) {
        let (form, close) = match result {
            OptionsResult::Open => return,
            OptionsResult::Accepted(form) => (Some(form), true),
            OptionsResult::Applied(form) => (Some(form), false),
            OptionsResult::Canceled => (None, true),
        };
        if let Some(form) = form {
            let old = self.state.settings.clone();
            self.state.settings = form.to_settings(&self.state.settings, &self.capture_dir);
            self.apply_language();
            self.apply_theme();
            self.sync_tray();
            // A changed shortcut Windows refuses keeps its previous keys.
            let results = self.shortcuts.apply(effective(&self.state.settings), &mut self.hotkeys);
            let refused: Vec<&str> =
                results.iter().filter(|(_, r)| r.is_err()).map(|(id, _)| *id).collect();
            if rollback(&mut self.state.settings, &old, &refused) {
                self.sync_shortcuts();
            }
            self.save_settings();
            self.sync_autostart();
            self.refresh_toolbar();
            let settings = self.state.settings.clone();
            if let Some(o) = &mut self.options {
                o.applied(&settings);
                if !refused.is_empty() {
                    // Options stays open to say which keys another app holds.
                    o.refused(&refused, &settings);
                }
            }
            if !refused.is_empty() {
                return;
            }
        }
        // Apply keeps the window; Cancel after it keeps what was applied.
        if !close {
            return;
        }
        self.options = None;
        // Cancel drops the theme being tried out.
        self.apply_theme();
    }

    /// The windows' look and size, from the settings or from Windows' app
    /// mode. While Options is open, its choices are tried out before OK.
    fn apply_theme(&mut self) {
        use gyozarec::settings::ThemeChoice;
        use gyozarec::uikit::theme;
        let choice = self.options.as_ref().map_or(self.state.settings.theme, |o| o.theme());
        let system_dark = self.system_theme == Some(winit::window::Theme::Dark);
        let look = choice.look(system_dark);
        // Only a dark theme picked by hand darkens title bars; otherwise they
        // stay Windows', which also reports its changes for Automatic.
        let code = u8::from(choice != ThemeChoice::Auto && look.is_dark());
        let size = self
            .options
            .as_ref()
            .map_or(self.state.settings.interface_size, |o| o.interface_size());
        let zoom = f32::from(theme::nearest_zoom(size)) / 100.0;
        let unchanged = TITLE_THEME.swap(code, Ordering::Relaxed) == code
            && theme::look() == look
            && theme::zoom() == zoom;
        theme::set_look(look);
        theme::set_zoom(size);
        if unchanged {
            return;
        }
        if let Some(t) = &self.toolbar {
            t.surface.window.set_theme(title_theme());
        }
        self.refresh_toolbar();
        if let Some(o) = &mut self.options {
            o.restyle();
        }
        for e in &mut self.editors {
            e.restyle();
        }
    }

    /// Switches the interface to the language the settings now ask for.
    fn apply_language(&mut self) {
        let language = resolve(self.state.settings.language(), &system_ui_languages());
        if language == self.state.language {
            return;
        }
        self.state.set_language(language);
        self.fonts = Fonts::for_language(language);
    }

    fn on_menu(&mut self, event_loop: &ActiveEventLoop, event: MenuEvent) {
        let id = event.id.0.as_str();
        if let Some(command) = tray::command(id) {
            match command {
                TrayCommand::Quick(action) => self.start_quick(action),
                TrayCommand::Convert => self.start_conversion(),
                TrayCommand::Show => self.activate(),
                TrayCommand::Options => {
                    self.activate();
                    self.open_options(event_loop);
                }
                TrayCommand::Exit => self.request_exit(event_loop),
            }
            return;
        }
        if id.starts_with(editor::MENU_PREFIX) {
            for e in &mut self.editors {
                e.on_menu(id);
            }
            return;
        }
        if id.starts_with(options::MENU_PREFIX) {
            if let Some(o) = &mut self.options {
                o.on_menu(id);
            }
            self.apply_theme();
            return;
        }
        if id == CONVERT_MENU_ID {
            self.start_conversion();
            return;
        }
        let index = |prefix: &str| id.strip_prefix(prefix).and_then(|v| v.parse::<usize>().ok());
        if let Some(area) = index("area:").and_then(|i| AREAS.get(i)) {
            self.state.settings.area = *area;
        } else if let Some(format) = index("format:").and_then(|i| FORMATS.get(i)) {
            // A lasso stays chosen: recordings use a rectangle meanwhile.
            self.state.settings.format = *format;
        } else if let Some(s) = id.strip_prefix("delay:").and_then(|v| v.parse::<u32>().ok()) {
            self.state.settings.delay_seconds = s;
        } else {
            return;
        }
        self.state.settings_changed();
        self.save_settings();
        self.refresh_toolbar();
    }

    fn on_ui(&mut self, event_loop: &ActiveEventLoop, req: UiRequest) {
        match req {
            UiRequest::HideToolbar => {
                if let Some(t) = &self.toolbar {
                    t.surface.window.set_visible(false);
                }
                // Editors stay out of the capture too.
                for e in &self.editors {
                    e.set_visible(false);
                }
            }
            UiRequest::ShowToolbar => {
                if let Some(t) = self.toolbar.as_ref().filter(|_| !self.toolbar_hidden) {
                    t.surface.window.set_visible(true);
                    if !self.focus_editor {
                        t.surface.window.focus_window();
                    }
                }
                for e in &self.editors {
                    e.set_visible(true);
                }
                if std::mem::take(&mut self.focus_editor)
                    && let Some(e) = self.editors.last()
                {
                    e.focus();
                }
            }
            UiRequest::Notify(n) => {
                if !self.exit_pending {
                    self.notice = None;
                    self.notice = notice::NoticeWindow::open(event_loop, n, self.state.texts());
                    let generation = self.notice_timer.start();
                    self.schedule_notice_timeout(generation);
                }
            }
            UiRequest::OpenEditor { image, path, on_disk, work_area, scale } => {
                if !self.exit_pending {
                    self.open_editor(event_loop, image, path, on_disk, Some(work_area), scale);
                    self.focus_editor = true;
                    if let Some(old) = self.replace_editor.take()
                        && let Some(i) = self.editors.iter().position(|e| e.id() == old)
                        && !self.editors[i].is_dirty()
                    {
                        self.editors.remove(i);
                    }
                }
            }
            UiRequest::Select { request, reply } => {
                if let Some(mut old) = self.overlay.take() {
                    old.cancel();
                }
                if !accepts(self.exit_pending, Incoming::Selection) {
                    let _ = reply.send(None);
                    return;
                }
                let cursor = Win32Cursor.position();
                // Shown only while picking a monitor; the area bar can switch to it.
                let hint = Some(self.state.texts().pick_monitor_hint);
                let session =
                    OverlaySession::open(event_loop, request, reply, cursor, hint, &self.fonts);
                // A session that could not open any window has already replied.
                self.overlay = (!session.is_finished()).then_some(session);
            }
            UiRequest::CaptureFinished(outcome) => {
                self.state.busy = false;
                self.replace_editor = None;
                if self.exit_pending {
                    self.stop_conversion();
                    event_loop.exit();
                    return;
                }
                self.state.apply_outcome(&outcome);
                self.refresh_toolbar();
            }
            UiRequest::RecordingStarted { .. }
                if !accepts(self.exit_pending, Incoming::Recording) =>
            {
                // Dropping the command channel makes the bridge cancel it.
            }
            UiRequest::RecordingStarted { region, monitor, max, commands } => {
                // If the panel cannot open, `commands` is dropped and the
                // recording is canceled by the bridge.
                self.recording = RecordingSession::open(
                    event_loop,
                    region,
                    &monitor,
                    max,
                    commands,
                    self.state.texts(),
                    &self.fonts,
                );
            }
            UiRequest::RecordingTick { elapsed, .. } => {
                if let Some(session) = &mut self.recording {
                    session.tick(elapsed, &self.fonts);
                }
            }
            UiRequest::RecordingEnded => self.recording = None,
        }
    }

    /// Brings GyozaRec forward, unless a capture is running.
    fn activate(&mut self) {
        match activation(self.state.busy, self.options.is_some()) {
            Activation::Nothing => {}
            Activation::FocusOptions => {
                if let Some(o) = &self.options {
                    o.focus();
                }
            }
            Activation::ShowToolbar => self.show_toolbar(),
        }
    }

    fn open_folder(&self) {
        if let Some(path) = &self.state.last_saved {
            gyozarec::platform::reveal_in_explorer(path);
        }
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.toolbar.is_none() {
            self.system_theme = event_loop.system_theme();
            self.apply_theme();
            self.sync_tray();
            let visible = startup_visible(self.autostart, self.tray.is_some());
            self.toolbar_hidden = !visible;
            self.toolbar = Some(Toolbar::create(event_loop, &self.state, &self.fonts, visible));
            self.sync_shortcuts();
            self.sync_autostart();
            #[cfg(feature = "github-updates")]
            self.check_for_updates();
            if let Some(action) = self.action_on_start.take() {
                self.start_quick(action);
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Ui(req) => self.on_ui(event_loop, req),
            AppEvent::Menu(event) => self.on_menu(event_loop, event),
            AppEvent::Shortcut(id) => {
                if let Some(action) = self.shortcuts.action_for(id) {
                    self.start_quick(action);
                }
            }
            AppEvent::TrayClick => self.activate(),
            AppEvent::Convert(event) => self.on_conversion(event),
            #[cfg(feature = "github-updates")]
            AppEvent::UpdateChecked(Some((version, page))) => {
                self.state.update_available(&version, page);
                self.refresh_toolbar();
            }
            #[cfg(feature = "github-updates")]
            AppEvent::UpdateChecked(None) => {}
            AppEvent::NoticeTimeout(generation) => {
                if self.notice_timer.expired(generation) {
                    self.notice = None;
                }
            }
            AppEvent::EditorSaved { window, version, result } => {
                if let Some(i) = self.editors.iter().position(|e| e.id() == window)
                    && self.editors[i].saved(version, result) == editor::EditorResult::Closed
                {
                    self.editors.remove(i);
                }
            }
            AppEvent::Instance(InstanceCommand::Quick(action)) => self.start_quick(action),
            AppEvent::Instance(InstanceCommand::Activate) => self.activate(),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, mut event: WindowEvent) {
        // Windows' app mode changed; only windows that follow it are told.
        if let WindowEvent::ThemeChanged(theme) = event {
            self.system_theme = Some(theme);
            self.apply_theme();
        }
        let pinned = self.overlay.as_ref().and_then(|o| o.surface(id));
        let pinned = pinned.or_else(|| self.recording.as_ref()?.surface(id));
        let pinned =
            pinned.or_else(|| self.notice.as_ref().filter(|n| n.id() == id).map(|n| n.surface()));
        if let Some(surface) = pinned {
            surface.keep_on_target(&mut event);
        }
        if let Some(n) = self.notice.as_mut().filter(|n| n.id() == id) {
            match n.handle(&event, &self.fonts) {
                notice::NoticeAction::Open => {
                    if let Some(n) = self.notice.take() {
                        self.open_notice(event_loop, n.notice);
                    }
                }
                notice::NoticeAction::Close => self.notice = None,
                notice::NoticeAction::Hover(inside) => {
                    if let Some(generation) = self.notice_timer.hover(inside) {
                        self.schedule_notice_timeout(generation);
                    }
                }
                notice::NoticeAction::None => {}
            }
            return;
        }
        if let Some(session) = self.overlay.as_mut().filter(|s| s.owns(id)) {
            session.handle(id, &event, &self.fonts);
            if session.is_finished() {
                // The area picked on the bar becomes the toolbar's, like the
                // Snipping Tool remembers its mode.
                if let Some(area) = session.switched_area() {
                    self.state.settings.area = area;
                    self.state.settings_changed();
                    self.save_settings();
                    self.refresh_toolbar();
                }
                self.overlay = None;
            }
            return;
        }
        if let Some(session) = self.recording.as_mut().filter(|s| s.owns(id)) {
            session.handle(id, &event, &self.fonts);
            return;
        }
        if let Some(i) = self.editors.iter().position(|e| e.id() == id) {
            match self.editors[i].handle(&event) {
                editor::EditorResult::Open => {}
                editor::EditorResult::Closed => {
                    self.editors.remove(i);
                }
                editor::EditorResult::NewCapture => {
                    self.replace_editor = Some(id);
                    self.start_quick(QuickAction::Image);
                }
                editor::EditorResult::OpenFile(path) => {
                    let scale = self.editors[i].scale_factor();
                    match gyozarec::editor::load(&path) {
                        Ok(img) => {
                            self.open_editor(event_loop, Arc::new(img), path, true, None, scale)
                        }
                        Err(err) => gyozarec::platform::show_error_box(&err.to_string()),
                    }
                }
            }
            return;
        }
        if let Some(options) = self.options.as_mut().filter(|o| o.id() == id) {
            let result = options.handle(&event);
            self.close_options(result);
            return;
        }
        let Some(t) = &mut self.toolbar else { return };
        if t.surface.window.id() != id {
            return;
        }
        if let WindowEvent::CloseRequested = event {
            match close_policy(self.tray.is_some()) {
                ClosePolicy::HideToTray => {
                    self.toolbar_hidden = true;
                    t.surface.window.set_visible(false);
                }
                ClosePolicy::Exit => self.request_exit(event_loop),
            }
            return;
        }
        let action = t.handle(&event, &self.fonts);
        if self.options.is_some() {
            return; // the Options window is modal
        }
        match action {
            Some(ToolbarAction::Capture) => self.start_quick(QuickAction::Current),
            Some(
                action @ (ToolbarAction::AreaMenu
                | ToolbarAction::FormatMenu
                | ToolbarAction::DelayMenu),
            ) => self.show_menu(action),
            Some(ToolbarAction::OpenFolder) => self.open_folder(),
            Some(ToolbarAction::Download) => {
                if let Some(page) = self.state.update.as_ref().and_then(|u| u.safe_page()) {
                    use gyozarec::capture::FileRevealer;
                    gyozarec::platform::ExplorerRevealer.open(std::path::Path::new(page));
                }
            }
            Some(ToolbarAction::CancelConversion) => {
                if let Some(c) = &self.conversion {
                    c.cancel.store(true, Ordering::SeqCst);
                }
            }
            Some(ToolbarAction::Options) => self.open_options(event_loop),
            Some(ToolbarAction::Zoom(step)) => {
                let s = &mut self.state.settings;
                s.interface_size = gyozarec::uikit::theme::step_zoom(s.interface_size, step);
                self.save_settings();
                self.apply_theme();
            }
            None => {}
        }
    }
}
