//! A winit window with a softbuffer surface.

use gyozarec::geometry::PixelSize;
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

pub struct WindowSurface {
    pub window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    _context: softbuffer::Context<Rc<Window>>,
}

impl WindowSurface {
    pub fn new(window: Window) -> WindowSurface {
        let window = Rc::new(window);
        let context = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        let surface =
            softbuffer::Surface::new(&context, window.clone()).expect("softbuffer surface");
        WindowSurface { window, surface, _context: context }
    }

    pub fn size(&self) -> PixelSize {
        let s = self.window.inner_size();
        PixelSize { width: s.width as i32, height: s.height as i32 }
    }

    /// Shows `pixels` (`0x00RRGGBB`, row-major at the current inner size).
    pub fn present(&mut self, pixels: &[u32]) {
        let s = self.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(s.width), NonZeroU32::new(s.height)) else {
            return;
        };
        if self.surface.resize(w, h).is_err() {
            return;
        }
        let Ok(mut buffer) = self.surface.buffer_mut() else { return };
        let n = buffer.len().min(pixels.len());
        buffer[..n].copy_from_slice(&pixels[..n]);
        let _ = buffer.present();
    }

    /// Native window handle (HWND) for Win32 calls.
    pub fn hwnd(&self) -> isize {
        match self.window.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
            _ => 0,
        }
    }
}
