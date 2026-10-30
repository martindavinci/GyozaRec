# Changelog

All notable changes to GyozaRec are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [0.4.5] - 2026-10-30

### Added
- Interface size in Options, next to Theme: 100 % to 200 % on top of Windows' display scale, for every window and the capture bar; it shows at once, like the theme. Ctrl + mouse wheel or Ctrl +/- on the toolbar change it too. Options itself grows only as far as it still fits the screen.
- The author's name next to the version at the bottom of Options.

## [0.4.4] - 2026-10-30

### Added
- Icons on the toolbar buttons: a camera to capture, a red dot to record, the area, the format (image, video, GIF), a timer for the delay and a cog for Options.
- Apply in Options, next to OK and Cancel: saves and applies the settings and keeps the window open; it is off until something changes.

### Changed
- Choosing a theme in Options shows it at once; Cancel goes back to the previous one.
- Themes have names: Gyoza (light), Slate (dark) and Toasted (dark, the former Dark). Automatic now uses Slate when Windows is dark.
- Gyoza (light) leaves the title bars to Windows, as before 0.4.2; only a dark theme picked by hand darkens them.

## [0.4.3] - 2026-10-30

### Added
- A third look, Slate (blue-grey dark), in "Theme" in Options.

### Changed
- The area bar during image captures is twice as large, easier to hit.

## [0.4.2] - 2026-10-30

### Added
- A dark theme: "Theme" in Options is Automatic (follows Windows' app mode, the default), Light or Dark; title bars follow it too.
- A bar at the top of the screen during image captures switches between Rectangle, Window and Full screen, or cancels; the area picked there becomes the toolbar's, and Full screen on a single monitor captures at once.

### Changed
- "Check for updates" moved next to OK and Cancel in Options, to make room for Theme.

## [0.4.1] - 2026-10-29

### Added
- "Image captures" in Options: save and open the editor (as before), only save, or copy and open the editor without saving (off by default); a capture that was never saved is marked "Not saved yet" in the editor's title and on the status line.
- "Editor: ask to save unsaved or edited images" in Options (on by default); off, the editor closes without asking.

### Changed
- "Check for updates" moved to the right column of Options, under Language.

## [0.4.0] - 2026-10-26

### Added
- An editor that opens after every image capture (on by default): arrow, rectangle, pen, highlighter, text, pixelate, redact and crop, with undo, redo, Save, Save as and Copy, New (another capture) and Open; every change is copied to the clipboard when copying is on.
- A notification with a thumbnail after each saved capture (on by default); clicking it opens the file, images in the editor.
- "Convert a video to GIF…" in the Format menu and the notification area menu, with progress and cancel.
- Once a day, a check for a newer release on GitHub (on by default), shown on the status line with a Download link.
- File names can be ten random characters instead of the date and time ("File names" in Options).

### Changed
- Date-and-time file names no longer start with "GyozaRec_" unless "Start with \"GyozaRec_\"" is turned on in Options.
- GIFs store only what changes between frames, so mostly still recordings are much smaller.

### Fixed
- System audio in MP4s no longer starts seconds late (with the start sound in it) on output devices whose packet timestamps run ahead of the system clock.
- Shortcut keys refused by Windows are reported in Options instead of vanishing silently.
- Restoring a default shortcut with Backspace, and pressing OK, no longer allow two shortcuts with the same keys.

## [0.3.0] - 2026-10-25

### Added
- System audio in MP4 recordings (on by default), captured from the default output device and encoded as AAC.
- Recording sounds for start, pause, resume and stop, timed so they stay out of the video.
- JPG screenshots, with a quality setting.
- With several monitors, full-screen captures ask which monitor to take.
- Start with Windows (off by default), hidden in the notification area; also for the portable version.
- Configurable global shortcuts: new keys, switch each one off, reset to the defaults.

### Changed
- "Sound on capture" is now "In-app sounds".

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

[0.4.5]: https://github.com/martindavinci/GyozaRec/compare/v0.4.4...v0.4.5
[0.4.4]: https://github.com/martindavinci/GyozaRec/compare/v0.4.3...v0.4.4
[0.4.3]: https://github.com/martindavinci/GyozaRec/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/martindavinci/GyozaRec/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/martindavinci/GyozaRec/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/martindavinci/GyozaRec/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/martindavinci/GyozaRec/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/martindavinci/GyozaRec/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/martindavinci/GyozaRec/releases/tag/v0.1.0
