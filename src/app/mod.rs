mod hotkeys;
mod options;
mod overlay;
mod recording;
mod surface;
mod toolbar;
mod tray;

use gyozarec::bridge::{
    BridgeRecordingUi, BridgeSelectionUi, BridgeToolbar, CaptureThread, UiRequest, UiSender,
};
use gyozarec::capture::{CaptureCoordinator, CursorLocator, ScreenGrabber};
use gyozarec::clock::{Clock, SystemClock};
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
use std::sync::{Arc, Mutex};
use toolbar::{Toolbar, ToolbarAction};
use tray::TrayCommand;
use tray_icon::TrayIcon;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::WindowId;

/// The gyoza icon for title bars and the taskbar (derived from GyozaRec.ico).
pub fn window_icon() -> Option<winit::window::Icon> {
    let png = include_bytes!("../../assets/icon/gyozarec-256.png");
    let img = image::load_from_memory(png).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    winit::window::Icon::from_rgba(img.into_raw(), w, h).ok()
}

/// Events delivered to the UI thread through the event loop proxy.
pub enum AppEvent {
    Ui(UiRequest),
    Menu(MenuEvent),
    Instance(InstanceCommand),
    /// A registered global shortcut was pressed (its hotkey id).
    Shortcut(u32),
    /// Left click on the notification icon.
    TrayClick,
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
        let tray_proxy = Mutex::new(proxy);
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

    /// Quits now, or as soon as the running capture has been cancelled.
    fn request_exit(&mut self, event_loop: &ActiveEventLoop) {
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
                .map(|(i, a)| (format!("area:{i}"), area_label(*a, texts), *a == s.area))
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
        self.options =
            OptionsWindow::open(event_loop, t.surface.hwnd(), form, self.state.language, lines);
    }

    fn close_options(&mut self, result: OptionsResult) {
        match result {
            OptionsResult::Open => return,
            OptionsResult::Accepted(form) => {
                let old = self.state.settings.clone();
                self.state.settings = form.to_settings(&self.state.settings, &self.capture_dir);
                self.apply_language();
                self.sync_tray();
                // A changed shortcut Windows refuses keeps its previous keys.
                let results =
                    self.shortcuts.apply(effective(&self.state.settings), &mut self.hotkeys);
                let refused: Vec<&str> =
                    results.iter().filter(|(_, r)| r.is_err()).map(|(id, _)| *id).collect();
                if rollback(&mut self.state.settings, &old, &refused) {
                    self.sync_shortcuts();
                }
                self.save_settings();
                self.sync_autostart();
                self.refresh_toolbar();
            }
            OptionsResult::Canceled => {}
        }
        self.options = None;
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
                TrayCommand::Show => self.activate(),
                TrayCommand::Options => {
                    self.activate();
                    self.open_options(event_loop);
                }
                TrayCommand::Exit => self.request_exit(event_loop),
            }
            return;
        }
        if id.starts_with(options::MENU_PREFIX) {
            if let Some(o) = &mut self.options {
                o.on_menu(id);
            }
            return;
        }
        let index = |prefix: &str| id.strip_prefix(prefix).and_then(|v| v.parse::<usize>().ok());
        if let Some(area) = index("area:").and_then(|i| AREAS.get(i)) {
            self.state.settings.area = *area;
        } else if let Some(format) = index("format:").and_then(|i| FORMATS.get(i)) {
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
            }
            UiRequest::ShowToolbar => {
                if let Some(t) = self.toolbar.as_ref().filter(|_| !self.toolbar_hidden) {
                    t.surface.window.set_visible(true);
                    t.surface.window.focus_window();
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
                let hint = (request.area == gyozarec::geometry::CaptureArea::FullScreen)
                    .then_some(self.state.texts().pick_monitor_hint);
                let session =
                    OverlaySession::open(event_loop, request, reply, cursor, hint, &self.fonts);
                // A session that could not open any window has already replied.
                self.overlay = (!session.is_finished()).then_some(session);
            }
            UiRequest::CaptureFinished(outcome) => {
                self.state.busy = false;
                if self.exit_pending {
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
            self.sync_tray();
            let visible = startup_visible(self.autostart, self.tray.is_some());
            self.toolbar_hidden = !visible;
            self.toolbar = Some(Toolbar::create(event_loop, &self.state, &self.fonts, visible));
            self.sync_shortcuts();
            self.sync_autostart();
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
            AppEvent::Instance(InstanceCommand::Quick(action)) => self.start_quick(action),
            AppEvent::Instance(InstanceCommand::Activate) => self.activate(),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, mut event: WindowEvent) {
        let pinned = self.overlay.as_ref().and_then(|o| o.surface(id));
        if let Some(surface) = pinned.or_else(|| self.recording.as_ref()?.surface(id)) {
            surface.keep_on_target(&mut event);
        }
        if let Some(session) = self.overlay.as_mut().filter(|s| s.owns(id)) {
            session.handle(id, &event, &self.fonts);
            if session.is_finished() {
                self.overlay = None;
            }
            return;
        }
        if let Some(session) = self.recording.as_mut().filter(|s| s.owns(id)) {
            session.handle(id, &event, &self.fonts);
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
            Some(ToolbarAction::Options) => self.open_options(event_loop),
            None => {}
        }
    }
}
