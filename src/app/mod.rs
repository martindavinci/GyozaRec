mod options;
mod overlay;
mod recording;
mod surface;
mod toolbar;

use gyozarec::bridge::{
    BridgeRecordingUi, BridgeSelectionUi, BridgeToolbar, CaptureThread, UiRequest, UiSender,
};
use gyozarec::capture::{CaptureCoordinator, CursorLocator, ScreenGrabber};
use gyozarec::clock::{Clock, SystemClock};
use gyozarec::i18n::resolve;
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
use gyozarec::quick::{QuickAction, apply, can_start};
use gyozarec::settings::SettingsStore;
use gyozarec::uikit::painter::Fonts;
use muda::{CheckMenuItem, ContextMenu, Menu, MenuEvent};
use options::{OptionsResult, OptionsWindow};
use overlay::OverlaySession;
use recording::RecordingSession;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use toolbar::{Toolbar, ToolbarAction};
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
}

impl App {
    pub fn new(proxy: EventLoopProxy<AppEvent>, action_on_start: Option<QuickAction>) -> App {
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
            }),
            sinks: Box::new(DefaultSinkFactory),
            clock,
        };
        let capture = CaptureThread::spawn(coordinator, sender);
        let menu_proxy = Mutex::new(proxy);
        MenuEvent::set_event_handler(Some(move |event| {
            let _ = menu_proxy.lock().unwrap().send_event(AppEvent::Menu(event));
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
            action_on_start,
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
        self.options = OptionsWindow::open(event_loop, t.surface.hwnd(), form, self.state.language);
    }

    fn close_options(&mut self, result: OptionsResult) {
        match result {
            OptionsResult::Open => return,
            OptionsResult::Accepted(form) => {
                self.state.settings = form.to_settings(&self.state.settings, &self.capture_dir);
                self.save_settings();
                self.apply_language();
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

    fn on_menu(&mut self, event: MenuEvent) {
        let id = event.id.0.as_str();
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
                if let Some(t) = &self.toolbar {
                    t.surface.window.set_visible(true);
                    t.surface.window.focus_window();
                }
            }
            UiRequest::Select { request, reply } => {
                if let Some(mut old) = self.overlay.take() {
                    old.cancel();
                }
                let cursor = Win32Cursor.position();
                self.overlay =
                    Some(OverlaySession::open(event_loop, request, reply, cursor, &self.fonts));
            }
            UiRequest::CaptureFinished(outcome) => {
                self.state.busy = false;
                self.state.apply_outcome(&outcome);
                self.refresh_toolbar();
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
    fn activate(&self) {
        match activation(self.state.busy, self.options.is_some()) {
            Activation::Nothing => {}
            Activation::FocusOptions => {
                if let Some(o) = &self.options {
                    o.focus();
                }
            }
            Activation::ShowToolbar => {
                if let Some(t) = &self.toolbar {
                    t.surface.window.set_minimized(false);
                    t.surface.window.set_visible(true);
                    t.surface.window.focus_window();
                }
            }
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
            self.toolbar = Some(Toolbar::create(event_loop, &self.state, &self.fonts));
            if let Some(action) = self.action_on_start.take() {
                self.start_quick(action);
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Ui(req) => self.on_ui(event_loop, req),
            AppEvent::Menu(event) => self.on_menu(event),
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
            event_loop.exit();
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
