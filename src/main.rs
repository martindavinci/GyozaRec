#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use gyozarec::instance::{self, Instance, InstanceCommand};
use gyozarec::presentation::StartupOptions;
use std::sync::Mutex;
use winit::event_loop::EventLoop;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = StartupOptions::parse(&args);
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "user".into());

    // A second launch hands its request to the running instance and exits.
    let listener = match instance::acquire(&instance::socket_name(&user)) {
        Ok(Instance::Secondary(client)) => {
            gyozarec::platform::allow_foreground_for_others();
            let cmd = if options.capture_now {
                InstanceCommand::Capture
            } else {
                InstanceCommand::Activate
            };
            let _ = client.send(cmd);
            return;
        }
        Ok(Instance::Primary(listener)) => Some(listener),
        Err(err) => {
            eprintln!("single instance unavailable: {err}");
            None
        }
    };

    let event_loop = EventLoop::<app::AppEvent>::with_user_event().build().expect("event loop");
    if let Some(listener) = listener {
        let proxy = Mutex::new(event_loop.create_proxy());
        listener.spawn(move |cmd| {
            let _ = proxy.lock().unwrap().send_event(app::AppEvent::Instance(cmd));
        });
    }
    let mut app = app::App::new(event_loop.create_proxy(), options.capture_now);
    if let Err(err) = event_loop.run_app(&mut app) {
        eprintln!("event loop error: {err}");
    }
}
