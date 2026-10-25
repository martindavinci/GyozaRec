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
- **Options** — save folder, copy to clipboard, sound, cursor in recordings, opening the folder after a recording, frame rate and maximum length of GIFs and videos, language.

<p align="center">
  <img src="docs/screenshots/toolbar.png" alt="The GyozaRec toolbar" valign="top">
  &nbsp;&nbsp;
  <img src="docs/screenshots/options.png" alt="The Options window" valign="top">
</p>

Images are copied to the clipboard; videos and GIFs are copied as files. A window recording keeps the area the window covered when recording started. While recording, a small panel shows the elapsed time with **Pause**/**Resume**, **Stop** and **Cancel** buttons; paused time is left out of the file. Nothing leaves your computer: no uploads, no accounts.

Running `gyozarec --capture` starts the selection right away; if the app is already open, the capture starts in the running instance.

The interface is available in English, Spanish, French, German, Italian, Brazilian Portuguese, Japanese, Korean and Simplified Chinese. The language follows Windows on first start and can be changed in **Options**.

<p align="center">
  <img src="docs/screenshots/languages.png" alt="The toolbar in Italian (window to MP4), Spanish (GIF with a 3-second delay), German, Japanese, Korean and Chinese">
</p>

## Download

From the repository's Releases page:

| Package | What it does |
|---|---|
| `GyozaRec_<version>_x64.msi` | Installs for the current user in `%LOCALAPPDATA%\Programs\GyozaRec`, without administrator rights, with a Start menu shortcut. Settings live in `%APPDATA%\GyozaRec`, captures in `Pictures\GyozaRec`. Uninstall from "Installed apps". |
| `GyozaRec-<version>-windows-x64-portable.zip` | Portable version: extract it and run `gyozarec.exe`. Settings and captures stay in the folder (`data\` and `Captures\`). |

Requires Windows 10 (version 2004 or later) or Windows 11, 64-bit. macOS and Linux are planned.

## Building

You need stable Rust (1.93 or later) with the MSVC toolchain.

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
