fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/whisper.rc");
        println!("cargo:rerun-if-changed=assets/whisper.ico");
        println!("cargo:rerun-if-changed=assets/whisper.manifest");
        #[cfg(target_os = "windows")]
        let _ = ();
        // embed-resource is a target.'cfg(windows)'.build-dependencies, wait:
        // Cargo evaluates target.'cfg(windows)'.build-dependencies against the HOST!
        // We'll move embed-resource to [build-dependencies] in Cargo.toml so cross-compilation compiles resources!
        embed_resource::compile("assets/whisper.rc", embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}
