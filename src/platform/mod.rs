//! Operating-system backends. Phase 1 ships Windows only.

#[cfg(windows)]
mod windows;

#[cfg(windows)]
#[allow(unused_imports)]
pub use self::windows::*;

#[cfg(not(windows))]
compile_error!("GyozaRec fase 1 supporta solo Windows (fasi 2–3: macOS, Linux)");
