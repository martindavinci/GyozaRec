use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId};

/// Events delivered to the UI thread through the event loop proxy.
#[derive(Debug)]
pub enum AppEvent {}

#[derive(Default)]
pub struct App {
    toolbar: Option<Window>,
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.toolbar.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("GyozaRec")
            .with_inner_size(LogicalSize::new(480.0, 120.0));
        match event_loop.create_window(attrs) {
            Ok(window) => self.toolbar = Some(window),
            Err(err) => {
                eprintln!("impossibile creare la finestra: {err}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let WindowEvent::CloseRequested = event {
            event_loop.exit();
        }
    }
}
