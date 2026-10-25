# Changelog

All notable changes to GyozaRec are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-10-25

### Added
- Notification area icon (on by default): closing the toolbar hides it; the icon's menu offers every capture, Options and Exit.
- Optional global shortcuts: Ctrl+Shift+F7 image, F8 full-screen image, F9 recording, F10 full-screen recording; optional Print Screen key. Options shows which ones another app already uses.
- Command-line options `--capture-image`, `--capture-fullscreen`, `--record` and `--record-fullscreen`, also forwarded to a running instance.
- Option to open each saved capture in its default app.
- The discard button on the recording panel asks for confirmation.
- Crash log: errors are saved to `crash.log` next to the settings.

### Changed
- GIFs are encoded on a separate thread, so large areas keep their frame rate.
- MP4 recordings larger than 4096×2304 are scaled down instead of failing.
- Release workflow: manual dry runs, narrower permissions, pinned packaging tool; optional automatic winget updates.

### Fixed
- A failure during a capture no longer leaves the app stuck with the toolbar hidden.
- A click in the selection before moving the mouse uses the real cursor position.
- The capture is cancelled, not stuck, if the selection screen cannot open.
- Launching GyozaRec again during a capture no longer shows the toolbar in the shot.
- Click-through windows are no longer offered in Window mode.
- The red recording frame keeps its exact size around very small areas.
- Settings saved with a UTF-8 byte order mark load correctly.

## [0.1.0] - 2026-10-25

### Added
- First release for Windows 10 and 11: rectangle, window or full-screen capture to PNG, MP4 or animated GIF, with delay, pause and resume, clipboard copy and optional folder opening.
- Interface in nine languages, detected from Windows.
- Per-user MSI installer and portable zip.

[0.2.0]: https://github.com/martindavinci/GyozaRec/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/martindavinci/GyozaRec/releases/tag/v0.1.0
