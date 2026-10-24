mod overlay;
mod recording;
mod surface;
mod toolbar;

use gyozarec::bridge::{
    BridgeRecordingUi, BridgeSelectionUi, BridgeToolbar, CaptureThread, UiRequest, UiSender,
};
use gyozarec::capture::{CaptureCoordinator, CursorLocator, ScreenGrabber};
use gyozarec::clock::{Clock, SystemClock};
use gyozarec::paths::AppPaths;
use gyozarec::platform::{
    DefaultSinkFactory, GdiGrabber, SystemSound, Win32Clipboard, Win32Cursor, Win32Monitors,
    Win32WindowList,
};
use gyozarec::presentation::{DELAY_OPTIONS, MODE_OPTIONS, ToolbarState};
use gyozarec::settings::SettingsStore;
use gyozarec::uikit::painter::Fonts;
use muda::{CheckMenuItem, ContextMenu, Menu, MenuEvent};
use overlay::OverlaySession;
use recording::RecordingSession;
use std::sync::{Arc, Mutex};
use toolbar::{Toolbar, ToolbarAction};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::window::WindowId;

/// Events delivered to the UI thread through the event loop proxy.
pub enum AppEvent {
    Ui(UiRequest),
    Menu(MenuEvent),
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
}

impl App {
    pub fn new(proxy: EventLoopProxy<AppEvent>) -> App {
        let paths = AppPaths::current();
        let store = SettingsStore::from_paths(&paths);
        let state = ToolbarState::new(store.load());
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
            fonts: Fonts::embedded(),
            store,
            state,
            capture,
            toolbar: None,
            overlay: None,
            recording: None,
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

    fn start_capture(&mut self) {
        if self.state.busy {
            return;
        }
        self.state.busy = true;
        if !self.capture.request(self.state.settings.clone()) {
            self.state.busy = false;
            self.state.set_error("il thread di cattura non risponde");
        }
        self.refresh_toolbar();
    }

    fn show_menu(&self, action: ToolbarAction) {
        let Some(t) = &self.toolbar else { return };
        let menu = Menu::new();
        if action == ToolbarAction::ModeMenu {
            for (i, (mode, label)) in MODE_OPTIONS.iter().enumerate() {
                let checked = *mode == self.state.settings.mode;
                let _ = menu.append(&CheckMenuItem::with_id(
                    format!("mode:{i}"),
                    label,
                    true,
                    checked,
                    None,
                ));
            }
        } else {
            for (seconds, label) in DELAY_OPTIONS {
                let checked = seconds == self.state.settings.delay_seconds;
                let _ = menu.append(&CheckMenuItem::with_id(
                    format!("delay:{seconds}"),
                    label,
                    true,
                    checked,
                    None,
                ));
            }
        }
        let anchor = t.menu_anchor(action);
        let position =
            muda::dpi::Position::Physical(muda::dpi::PhysicalPosition::new(anchor.x, anchor.y));
        // SAFETY: the HWND belongs to the live toolbar window on this thread.
        unsafe {
            menu.show_context_menu_for_hwnd(t.surface.hwnd(), Some(position));
        }
    }

    fn on_menu(&mut self, event: MenuEvent) {
        let id = event.id.0.as_str();
        if let Some(i) = id.strip_prefix("mode:").and_then(|v| v.parse::<usize>().ok()) {
            if let Some((mode, _)) = MODE_OPTIONS.get(i) {
                self.state.settings.mode = *mode;
            }
        } else if let Some(s) = id.strip_prefix("delay:").and_then(|v| v.parse::<u32>().ok()) {
            self.state.settings.delay_seconds = s;
        } else {
            return;
        }
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

    fn open_folder(&self) {
        if let Some(path) = &self.state.last_saved {
            let _ = std::process::Command::new("explorer.exe")
                .arg(format!("/select,{}", path.display()))
                .spawn();
        }
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.toolbar.is_none() {
            self.toolbar = Some(Toolbar::create(event_loop, &self.state, &self.fonts));
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Ui(req) => self.on_ui(event_loop, req),
            AppEvent::Menu(event) => self.on_menu(event),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
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
        let Some(t) = &mut self.toolbar else { return };
        if t.surface.window.id() != id {
            return;
        }
        if let WindowEvent::CloseRequested = event {
            event_loop.exit();
            return;
        }
        match t.handle(&event, &self.fonts) {
            Some(ToolbarAction::Capture) => self.start_capture(),
            Some(action @ (ToolbarAction::ModeMenu | ToolbarAction::DelayMenu)) => {
                self.show_menu(action)
            }
            Some(ToolbarAction::OpenFolder) => self.open_folder(),
            Some(ToolbarAction::Options) | None => {}
        }
    }
}
