fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/whisper.rc");
        println!("cargo:rerun-if-changed=assets/whisper.ico");
        println!("cargo:rerun-if-changed=assets/whisper.manifest");
        embed_resource::compile("assets/whisper.rc", embed_resource::NONE)
            .manifest_required()
            .unwrap();
    }
}
