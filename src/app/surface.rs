//! A winit window with a softbuffer surface.

use gyozarec::geometry::{PixelRect, PixelSize};
use std::cell::Cell;
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

pub struct WindowSurface {
    pub window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    _context: softbuffer::Context<Rc<Window>>,
    /// Exact physical geometry this window must keep (overlays, recording UI).
    target: Option<PixelRect>,
    /// A DPI change moved the window; put it back on the next move/resize.
    repin: Cell<bool>,
}

impl WindowSurface {
    pub fn new(window: Window) -> WindowSurface {
        let window = Rc::new(window);
        let context = softbuffer::Context::new(window.clone()).expect("softbuffer context");
        let surface =
            softbuffer::Surface::new(&context, window.clone()).expect("softbuffer surface");
        WindowSurface { window, surface, _context: context, target: None, repin: Cell::new(false) }
    }

    /// A window locked to `rect` in physical pixels. Windows creates every
    /// window on the primary monitor first, so moving it to a monitor with
    /// another DPI rescales it; the geometry is applied again here and kept
    /// by `keep_on_target`.
    pub fn pinned(window: Window, rect: PixelRect) -> WindowSurface {
        let mut surface = WindowSurface::new(window);
        surface.target = Some(rect);
        surface.apply_target();
        surface
    }

    fn apply_target(&self) {
        let Some(t) = self.target else { return };
        self.window.set_outer_position(PhysicalPosition::new(t.x, t.y));
        let _ = self
            .window
            .request_inner_size(PhysicalSize::new(t.width.max(1) as u32, t.height.max(1) as u32));
    }

    /// Undoes DPI-driven resizing and moving of a pinned window.
    pub fn keep_on_target(&self, event: &mut WindowEvent) {
        let Some(t) = self.target else { return };
        match event {
            WindowEvent::ScaleFactorChanged { inner_size_writer, .. } => {
                let size = PhysicalSize::new(t.width.max(1) as u32, t.height.max(1) as u32);
                let _ = inner_size_writer.request_inner_size(size);
                self.repin.set(true);
            }
            WindowEvent::Moved(_) | WindowEvent::Resized(_) if self.repin.replace(false) => {
                self.apply_target();
            }
            _ => {}
        }
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
