#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use winit::event_loop::EventLoop;

fn main() {
    let event_loop = EventLoop::<app::AppEvent>::with_user_event().build().expect("event loop");
    let mut app = app::App::new(event_loop.create_proxy());
    if let Err(err) = event_loop.run_app(&mut app) {
        eprintln!("errore dell'event loop: {err}");
    }
}
