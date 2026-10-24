//! Build tasks: `cargo xtask dist` makes the installer and the portable zip.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// A release tag must be `v` + the package version.
fn check_tag(tag: &str, version: &str) -> Result<(), String> {
    if tag == format!("v{version}") {
        Ok(())
    } else {
        Err(format!("tag {tag} non corrisponde alla versione {version}"))
    }
}

fn current_version() -> Result<String, String> {
    let manifest = String::from_utf8(read(&workspace_root().join("Cargo.toml"))?)
        .map_err(|e| e.to_string())?;
    package_version(&manifest).ok_or_else(|| "versione non trovata in Cargo.toml".into())
}

fn portable_zip_name(version: &str) -> String {
    format!("GyozaRec-{version}-windows-x64-portable.zip")
}

/// The first `version = "…"` line, i.e. the `[package]` version.
fn package_version(cargo_toml: &str) -> Option<String> {
    cargo_toml.lines().find_map(|line| {
        let value = line.trim().strip_prefix("version")?.trim_start().strip_prefix('=')?;
        Some(value.trim().trim_matches('"').to_string())
    })
}

/// cargo-packager names the MSI after the crate and the language
/// (`gyozarec_0.1.0_x64_it-IT.msi`); the published name is `GyozaRec_0.1.0_x64.msi`.
fn installer_display_name(file: &str) -> Option<String> {
    let stem = file.strip_prefix("gyozarec_")?.strip_suffix(".msi")?;
    let without_language = stem.rsplit_once('_').map_or(stem, |(rest, _)| rest);
    Some(format!("GyozaRec_{without_language}.msi"))
}

/// Text files for Notepad: CRLF line endings.
fn to_crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

/// `gyozarec.exe` first, then `extra` files in order, Deflate-compressed.
fn write_portable_zip(out: &Path, exe: &Path, extra: &[(&str, &[u8])]) -> std::io::Result<()> {
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut zip = zip::ZipWriter::new(std::fs::File::create(out)?);
    zip.start_file("gyozarec.exe", options)?;
    zip.write_all(&std::fs::read(exe)?)?;
    for (name, data) in extra {
        zip.start_file(*name, options)?;
        zip.write_all(data)?;
    }
    zip.finish()?;
    Ok(())
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask lives in the workspace").into()
}

fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let status = Command::new("cargo")
        .args(args)
        .current_dir(root)
        .status()
        .map_err(|e| format!("cargo {}: {e}", args.join(" ")))?;
    if status.success() { Ok(()) } else { Err(format!("cargo {} non riuscito", args.join(" "))) }
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn dist() -> Result<(), String> {
    let root = workspace_root();
    run(&root, &["build", "--release", "-p", "gyozarec"])?;
    let manifest = String::from_utf8(read(&root.join("Cargo.toml"))?).map_err(|e| e.to_string())?;
    let version = package_version(&manifest).ok_or("versione non trovata in Cargo.toml")?;
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist).map_err(|e| e.to_string())?;

    let text = |p: &str| -> Result<Vec<u8>, String> {
        Ok(to_crlf(&String::from_utf8_lossy(&read(&root.join(p))?)).into_bytes())
    };
    let (readme, license, ofl) =
        (text("packaging/LEGGIMI.txt")?, text("LICENSE")?, text("assets/fonts/OFL.txt")?);
    let extra: [(&str, &[u8]); 4] =
        [("portable", b""), ("LEGGIMI.txt", &readme), ("LICENSE.txt", &license), ("OFL.txt", &ofl)];
    let zip_path = dist.join(portable_zip_name(&version));
    let exe = root.join("target").join("release").join("gyozarec.exe");
    write_portable_zip(&zip_path, &exe, &extra).map_err(|e| e.to_string())?;

    let packager = Command::new("cargo").args(["packager", "--version"]).output();
    if !packager.is_ok_and(|o| o.status.success()) {
        return Err("cargo-packager mancante: cargo install cargo-packager --locked".into());
    }
    run(&root, &["packager", "--release", "--formats", "wix"])?;
    for entry in std::fs::read_dir(&dist).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(display) = installer_display_name(&name) {
            std::fs::rename(entry.path(), dist.join(display)).map_err(|e| e.to_string())?;
        }
    }

    println!("Pacchetti in {}:", dist.display());
    for entry in std::fs::read_dir(&dist).map_err(|e| e.to_string())?.flatten() {
        if entry.path().is_dir() {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        println!("  {} ({} KB)", entry.file_name().to_string_lossy(), size / 1024);
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("dist") => dist(),
        Some("check-tag") => match args.get(1) {
            Some(tag) => current_version().and_then(|v| check_tag(tag, &v)),
            None => Err("uso: cargo xtask check-tag <tag>".into()),
        },
        _ => Err("uso: cargo xtask dist | cargo xtask check-tag <tag>".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("{msg}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_tag_accepts_matching_version() {
        assert_eq!(check_tag("v0.1.0", "0.1.0"), Ok(()));
    }

    #[test]
    fn check_tag_rejects_mismatch() {
        assert_eq!(
            check_tag("v0.2.0", "0.1.0"),
            Err("tag v0.2.0 non corrisponde alla versione 0.1.0".to_string())
        );
        assert!(check_tag("0.1.0", "0.1.0").is_err());
    }

    #[test]
    fn portable_zip_name_has_version_and_platform() {
        assert_eq!(portable_zip_name("0.1.0"), "GyozaRec-0.1.0-windows-x64-portable.zip");
    }

    #[test]
    fn installer_name_gets_product_capitalization() {
        assert_eq!(
            installer_display_name("gyozarec_0.1.0_x64_it-IT.msi"),
            Some("GyozaRec_0.1.0_x64.msi".to_string())
        );
        assert_eq!(installer_display_name("GyozaRec-0.1.0-windows-x64-portable.zip"), None);
    }

    #[test]
    fn package_version_reads_first_version_line() {
        let toml = "[package]\nname = \"gyozarec\"\nversion = \"1.2.3\"\n\n[workspace]\n";
        assert_eq!(package_version(toml), Some("1.2.3".to_string()));
    }

    #[test]
    fn portable_zip_contains_exe_marker_and_texts() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("gyozarec.exe");
        std::fs::write(&exe, b"MZ").unwrap();
        let out = d.path().join("p.zip");
        let extra: [(&str, &[u8]); 4] =
            [("portable", b""), ("LEGGIMI.txt", b"x"), ("LICENSE.txt", b"m"), ("OFL.txt", b"y")];
        write_portable_zip(&out, &exe, &extra).unwrap();
        let mut z = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        let names: Vec<String> =
            (0..z.len()).map(|i| z.by_index(i).unwrap().name().unwrap().to_string()).collect();
        assert_eq!(names, ["gyozarec.exe", "portable", "LEGGIMI.txt", "LICENSE.txt", "OFL.txt"]);
    }
}
