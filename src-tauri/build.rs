fn main() {
    // On iOS, the Rust crate is compiled to a dylib and linked first; Swift
    // sources (WhispIntent.swift) compile later in the same Xcode build. Allow
    // the dylib to reference Swift @_cdecl symbols that get resolved at the
    // final app link step (whisp_present_shortcut_installer).
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target == "ios" {
        println!("cargo:rustc-link-arg=-Wl,-undefined,dynamic_lookup");
    }
    tauri_build::build()
}
