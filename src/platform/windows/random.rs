//! Unpredictable bytes from the system's random number generator.

use windows::Win32::Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom};

/// Fills `buf`; the system generator does not fail for small buffers.
pub fn random_bytes(buf: &mut [u8]) {
    // SAFETY: `buf` is a valid, writable slice for the whole call.
    let status = unsafe { BCryptGenRandom(None, buf, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    assert!(status.is_ok(), "system random generator failed: {status:?}");
}
