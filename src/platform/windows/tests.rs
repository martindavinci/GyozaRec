//! Tests against the real desktop; run with `cargo test -- --include-ignored`.

use super::*;
use crate::capture::{MonitorProvider, ScreenGrabber, WindowSnapshotter};
use crate::geometry::{PixelPoint, PixelRect};

#[test]
#[ignore = "desktop"]
fn monitors_include_primary_and_fit_virtual_screen() {
    let m = Win32Monitors;
    let ms = m.monitors();
    assert!(ms.iter().any(|x| x.bounds.contains(PixelPoint { x: 0, y: 0 })));
    for x in &ms {
        assert_eq!(x.bounds.intersect(&m.virtual_screen()), x.bounds);
        assert!(x.scale >= 1.0);
    }
}

#[test]
#[ignore = "desktop"]
fn grab_returns_requested_region_opaque() {
    let b = GdiGrabber.grab(PixelRect::new(0, 0, 64, 48), false).unwrap();
    assert_eq!((b.bounds, b.bgra.len()), (PixelRect::new(0, 0, 64, 48), 64 * 48 * 4));
    assert!(b.bgra.chunks(4).all(|px| px[3] == 255));
}

#[test]
#[ignore = "desktop"]
fn grab_with_cursor_succeeds() {
    let b = GdiGrabber.grab(PixelRect::new(0, 0, 64, 48), true).unwrap();
    assert_eq!(b.bounds, PixelRect::new(0, 0, 64, 48));
}

#[test]
#[ignore = "desktop"]
fn window_snapshot_has_only_non_empty_bounds() {
    let list = Win32WindowList { exclude_pid: std::process::id() }.snapshot();
    assert!(list.iter().all(|w| !w.bounds.is_empty()));
}

#[test]
#[ignore = "desktop"]
fn system_ui_languages_are_locale_tags() {
    let langs = system_ui_languages();
    assert!(!langs.is_empty());
    assert!(langs.iter().all(|l| l.len() >= 2 && !l.contains('\0')), "{langs:?}");
}
