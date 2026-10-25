//! Asks GitHub for GyozaRec's latest release over WinHTTP.

use crate::update::{RELEASES_HOST, RELEASES_PATH, ReleaseSource};
use core::ffi::c_void;
use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_OPTION_REDIRECT_POLICY, WINHTTP_OPTION_REDIRECT_POLICY_NEVER,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption,
    WinHttpSetTimeouts,
};
use windows::core::{HSTRING, PCWSTR};

/// Gives up on a slow network after this long, per step.
const TIMEOUT_MS: i32 = 10_000;
/// A release description is a few kilobytes; anything far bigger is not one.
const MAX_BODY: usize = 1 << 20;

/// The latest release from `api.github.com`; redirects elsewhere are refused.
pub struct GitHubReleases;

/// A WinHTTP handle, closed when dropped.
struct Handle(*mut c_void);

impl Handle {
    fn new(raw: *mut c_void) -> crate::Result<Handle> {
        if raw.is_null() {
            return Err(windows::core::Error::from_thread().into());
        }
        Ok(Handle(raw))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: a handle WinHTTP gave us, closed once.
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

impl ReleaseSource for GitHubReleases {
    fn latest_json(&self) -> crate::Result<String> {
        let agent = HSTRING::from(format!("GyozaRec/{}", env!("CARGO_PKG_VERSION")));
        // SAFETY: every handle is checked and closed by `Handle`; strings and
        // buffers outlive the calls that use them.
        unsafe {
            let session = Handle::new(WinHttpOpen(
                &agent,
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            ))?;
            WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS)?;
            let host = HSTRING::from(RELEASES_HOST);
            let connect =
                Handle::new(WinHttpConnect(session.0, &host, INTERNET_DEFAULT_HTTPS_PORT, 0))?;
            let path = HSTRING::from(RELEASES_PATH);
            let request = Handle::new(WinHttpOpenRequest(
                connect.0,
                &HSTRING::from("GET"),
                &path,
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ))?;
            let never = WINHTTP_OPTION_REDIRECT_POLICY_NEVER.to_ne_bytes();
            WinHttpSetOption(Some(request.0), WINHTTP_OPTION_REDIRECT_POLICY, Some(&never))?;
            let headers: Vec<u16> = "Accept: application/vnd.github+json".encode_utf16().collect();
            WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0)?;
            WinHttpReceiveResponse(request.0, std::ptr::null_mut())?;
            let (mut status, mut size) = (0u32, 4u32);
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some(&mut status as *mut u32 as *mut c_void),
                &mut size,
                std::ptr::null_mut(),
            )?;
            if status != 200 {
                return Err(crate::Error::Message(format!("GitHub answered {status}")));
            }
            let mut body = Vec::new();
            loop {
                let mut available = 0u32;
                WinHttpQueryDataAvailable(request.0, &mut available)?;
                if available == 0 {
                    break;
                }
                let start = body.len();
                body.resize(start + available as usize, 0);
                let mut read = 0u32;
                WinHttpReadData(
                    request.0,
                    body[start..].as_mut_ptr().cast(),
                    available,
                    &mut read,
                )?;
                body.truncate(start + read as usize);
                if body.len() > MAX_BODY {
                    return Err(crate::Error::Message("release description too large".into()));
                }
            }
            String::from_utf8(body).map_err(|e| crate::Error::Message(e.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs the internet; GitHub may also answer 403 when rate-limited.
    #[test]
    #[ignore = "network"]
    fn github_answers_with_a_release() {
        let json = GitHubReleases.latest_json().unwrap();
        let release = crate::update::parse_latest(&json).expect("a release");
        assert!(release.version >= crate::update::Version(0, 1, 0));
    }
}
