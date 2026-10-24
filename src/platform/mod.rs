//! Operating-system backends. Phase 1 ships Windows only.

#[cfg(windows)]
mod windows;

#[cfg(windows)]
#[allow(unused_imports)]
pub use self::windows::*;

#[cfg(not(windows))]
compile_error!("GyozaRec currently supports Windows only (macOS and Linux are planned)");
