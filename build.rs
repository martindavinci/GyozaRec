fn main() {
    println!("cargo:rerun-if-changed=res");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // Link the resource into the `gyozarec` binary only: a plain `compile`
        // can emit `rustc-link-lib=gyozarec`, which clashes with the library
        // crate of the same name and gets dropped.
        embed_resource::compile_for("res/gyozarec.rc", ["gyozarec"], embed_resource::NONE)
            .manifest_required()
            .unwrap();
    }
}
