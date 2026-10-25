use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=res");
    println!("cargo:rerun-if-changed=Cargo.toml");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let rc = Path::new(&std::env::var("OUT_DIR").unwrap()).join("gyozarec.rc");
        std::fs::write(&rc, resource_script()).unwrap();
        // Link the resource into the `gyozarec` binary only: a plain `compile`
        // can emit `rustc-link-lib=gyozarec`, which clashes with the library
        // crate of the same name and gets dropped.
        embed_resource::compile_for(&rc, ["gyozarec"], embed_resource::NONE)
            .manifest_required()
            .unwrap();
    }
}

/// Icon, manifest and the version block shown in the file's Properties.
fn resource_script() -> String {
    let env = |key: &str| std::env::var(key).unwrap();
    let res = Path::new(&env("CARGO_MANIFEST_DIR")).join("res");
    let path = |file: &str| res.join(file).to_string_lossy().replace('\\', "/");
    let version = env("CARGO_PKG_VERSION");
    let number = |i: usize| -> u16 {
        let core = version.split(['-', '+']).next().unwrap_or_default();
        core.split('.').nth(i).and_then(|n| n.parse().ok()).unwrap_or(0)
    };
    let csv = format!("{},{},{},0", number(0), number(1), number(2));
    let author = env("CARGO_PKG_AUTHORS");
    let description = env("CARGO_PKG_DESCRIPTION");
    format!(
        r#"#pragma code_page(65001)
1 ICON "{icon}"
1 24 "{manifest}"

1 VERSIONINFO
FILEVERSION {csv}
PRODUCTVERSION {csv}
FILEFLAGSMASK 0x3F
FILEFLAGS 0
FILEOS 0x40004
FILETYPE 1
FILESUBTYPE 0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "{author}"
      VALUE "FileDescription", "GyozaRec"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "gyozarec"
      VALUE "LegalCopyright", "Copyright © 2026 {author}. MIT License."
      VALUE "OriginalFilename", "gyozarec.exe"
      VALUE "ProductName", "GyozaRec"
      VALUE "ProductVersion", "{version}"
      VALUE "Comments", "{description}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = path("GyozaRec.ico"),
        manifest = path("gyozarec.manifest"),
    )
}
