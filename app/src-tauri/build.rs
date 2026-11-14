use std::path::PathBuf;

fn main() {
    tauri_build::build();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu")
    {
        shadow_default_manifest();
    }
}

/// MinGW gcc links `default-manifest.o` into every executable when the toolchain ships one.
/// Some builds (WinLibs) give it the same id and language as the manifest tauri-build embeds,
/// so ld cannot drop it and leaves two `RT_MANIFEST` entries, and Windows may apply the wrong
/// one. gcc looks for that object in `-B` directories first, so an empty one there keeps
/// Tauri's manifest the only one.
fn shadow_default_manifest() {
    let dir = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("no-default-manifest");
    std::fs::create_dir_all(&dir).unwrap();
    let machine: u16 = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86") => 0x014c,
        Ok("aarch64") => 0xaa64,
        _ => 0x8664,
    };
    // A COFF file header with no sections and no symbols: a valid object that adds nothing.
    let mut object = [0u8; 20];
    object[..2].copy_from_slice(&machine.to_le_bytes());
    std::fs::write(dir.join("default-manifest.o"), object).unwrap();
    println!("cargo:rustc-link-arg-bins=-B{}", dir.display());
}
