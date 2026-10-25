<p align="center">
  <img src="assets/icon/gyozarec-256.png" width="256" height="256" alt="GyozaRec">
</p>

<h1 align="center">GyozaRec</h1>

<p align="center">Fast, cute screen capture: PNG, GIF and MP4, entirely local.</p>

<p align="center">
  <a href="https://github.com/martindavinci/GyozaRec/actions/workflows/ci.yml"><img src="https://github.com/martindavinci/GyozaRec/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/martindavinci/GyozaRec/releases/latest"><img src="https://img.shields.io/github/v/release/martindavinci/GyozaRec" alt="Latest release"></a>
  <a href="https://github.com/martindavinci/GyozaRec/releases"><img src="https://img.shields.io/github/downloads/martindavinci/GyozaRec/total" alt="Downloads"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/martindavinci/GyozaRec" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D6" alt="Platform: Windows 10 | 11">
</p>

## Features

A small toolbar:

- **Capture** (**Record** for videos and GIFs) — starts a capture with the selected area, format and delay.
- **Area** — Rectangle, Window or Full screen.
- **Format** — PNG image, MP4 video or Animated GIF. Any area works with any format.
- **Delay** — from no delay up to 5 seconds.
- **Options** — save folder, copy to clipboard, sound, cursor in recordings, opening the folder or the file after a capture, notification area, global shortcuts, frame rate and maximum length of GIFs and videos, language.

<p align="center">
  <img src="docs/screenshots/toolbar.png" alt="The GyozaRec toolbar" valign="top">
  &nbsp;&nbsp;
  <img src="docs/screenshots/options.png" alt="The Options window" valign="top">
</p>

Images are copied to the clipboard; videos and GIFs are copied as files. A window recording keeps the area the window covered when recording started. While recording, a small panel shows the elapsed time with **Pause**/**Resume**, **Stop** and **Discard** buttons; paused time is left out of the file, and discarding asks for confirmation first. Nothing leaves your computer: no uploads, no accounts.

### Notification area and shortcuts

GyozaRec stays in the notification area: closing the toolbar hides it, a click on the icon brings it back, and the icon's menu offers every kind of capture, **Options** and **Exit**. Turn this off in **Options** to make closing the toolbar quit the app.

Global shortcuts are off by default; turn them on in **Options**:

| Shortcut | Action |
|---|---|
| Ctrl+Shift+F7 | Capture an image (current area) |
| Ctrl+Shift+F8 | Capture the full screen |
| Ctrl+Shift+F9 | Record (current area; MP4 unless GIF is selected) |
| Ctrl+Shift+F10 | Record the full screen |
| Print Screen | Capture an image (optional; Windows' own Print Screen shortcut must be turned off) |

If another app already uses a combination, **Options** marks it as unavailable.

### Command line

| Option | Starts |
|---|---|
| `--capture` | a capture with the toolbar's current settings |
| `--capture-image` | an image of the current area |
| `--capture-fullscreen` | an image of the full screen |
| `--record` | a recording of the current area |
| `--record-fullscreen` | a recording of the full screen |

If GyozaRec is already running, the capture starts in that instance. A shortcut to `gyozarec.exe --record` starts recording with one click.

The interface is available in English, Spanish, French, German, Italian, Brazilian Portuguese, Japanese, Korean and Simplified Chinese. The language follows Windows on first start and can be changed in **Options**. Corrections from native speakers are welcome.

<p align="center">
  <img src="docs/screenshots/languages.png" alt="The toolbar in Italian (window to MP4), Spanish (GIF with a 3-second delay), German, Japanese, Korean and Chinese">
</p>

## Download

From the [Releases page](https://github.com/martindavinci/GyozaRec/releases/latest):

| Package | What it does |
|---|---|
| `GyozaRec_<version>_x64.msi` | Installs for the current user in `%LOCALAPPDATA%\Programs\GyozaRec`, without administrator rights, with a Start menu shortcut. Settings live in `%APPDATA%\GyozaRec`, captures in `Pictures\GyozaRec`. Uninstall from "Installed apps". |
| `GyozaRec-<version>-windows-x64-portable.zip` | Portable version: extract it and run `gyozarec.exe`. Settings and captures stay in the folder (`data\` and `Captures\`). |

Requires Windows 10 (version 2004 or later) or Windows 11, 64-bit. macOS and Linux are planned.

If GyozaRec ever stops because of an error, the details are saved to `crash.log` next to the settings; attaching it to a bug report helps.

**Accessibility:** the interface is drawn by GyozaRec itself, so screen readers cannot read it yet and the windows do not support moving between controls with Tab.

## Building

You need Rust 1.99 or later with the MSVC toolchain; `rust-toolchain.toml` selects the exact version used for releases.

```
cargo build --release
cargo test
```

To build the MSI installer and the portable zip (requires `cargo install cargo-packager --locked`; cargo-packager downloads WiX, which needs .NET Framework 3.5):

```
cargo xtask dist
```

## License

MIT, see [`LICENSE`](LICENSE). The Nunito font is distributed under the SIL Open Font License, see [`assets/fonts/OFL.txt`](assets/fonts/OFL.txt).
