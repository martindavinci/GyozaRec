//! Crash reports: every panic is appended to `crash.log`; a panic on the main
//! thread, which ends the app, also tells the user where the report is.

use chrono::NaiveDateTime;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::ThreadId;

pub const LOG_FILE: &str = "crash.log";

/// One report, ready to be appended to the log.
pub fn report(
    message: &str,
    location: Option<&str>,
    thread: &str,
    version: &str,
    now: NaiveDateTime,
    backtrace: &str,
) -> String {
    format!(
        "=== GyozaRec {version} crashed at {} ===\nthread: {thread}\nlocation: {}\nmessage: {message}\nbacktrace:\n{backtrace}\n\n",
        now.format("%Y-%m-%d %H:%M:%S"),
        location.unwrap_or("unknown"),
    )
}

/// Appends `text` to `dir/crash.log`, creating the folder if needed.
pub fn append(dir: &Path, text: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(LOG_FILE);
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
    file.write_all(text.as_bytes())?;
    Ok(path)
}

static MAIN_THREAD: OnceLock<ThreadId> = OnceLock::new();
static SHOWN: AtomicBool = AtomicBool::new(false);

/// Installs the panic hook. `message` may contain `{path}`, replaced with the
/// log file's path; it is shown only for panics on the calling (main) thread.
pub fn install(log_dir: PathBuf, message: &'static str) {
    let _ = MAIN_THREAD.set(std::thread::current().id());
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let text = payload_text(info.payload());
        let location = info.location().map(|l| format!("{}:{}", l.file(), l.line()));
        let current = std::thread::current();
        let thread = current.name().unwrap_or("unnamed");
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let now = chrono::Local::now().naive_local();
        let entry =
            report(&text, location.as_deref(), thread, env!("CARGO_PKG_VERSION"), now, &backtrace);
        let path = append(&log_dir, &entry).unwrap_or_else(|_| log_dir.join(LOG_FILE));
        let on_main = MAIN_THREAD.get() == Some(&current.id());
        if on_main && !SHOWN.swap(true, Ordering::SeqCst) {
            crate::platform::show_error_box(
                &message.replace("{path}", &path.display().to_string()),
            );
        }
        previous(info);
    }));
}

fn payload_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-text panic".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn report_contains_version_location_thread_and_message() {
        let now = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap().and_hms_opt(14, 5, 9).unwrap();
        let text =
            report("boom", Some("src/app/mod.rs:42"), "gyozarec-capture", "0.2.0", now, "bt");
        for part in
            ["0.2.0", "2026-10-25 14:05:09", "gyozarec-capture", "src/app/mod.rs:42", "boom"]
        {
            assert!(text.contains(part), "{part} missing in {text}");
        }
    }

    #[test]
    fn report_is_appended_to_crash_log() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("data");
        append(&dir, "first\n").unwrap();
        let path = append(&dir, "second\n").unwrap();
        assert_eq!(path, dir.join(LOG_FILE));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "first\nsecond\n");
    }

    #[test]
    fn payload_text_reads_str_and_string() {
        assert_eq!(payload_text(&"a"), "a");
        assert_eq!(payload_text(&String::from("b")), "b");
        assert_eq!(payload_text(&5u8), "non-text panic");
    }
}
