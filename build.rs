fn main() {
    println!("cargo:rerun-if-changed=assets/facelock.rc");
    println!("cargo:rerun-if-changed=assets/facelock.manifest");
    println!("cargo:rerun-if-changed=assets/facelock.ico");
    // Icône + manifeste (DPI, UTF-8) : uniquement pour les binaires Windows.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/facelock.rc", embed_resource::NONE)
            .manifest_required()
            .unwrap();
    }
}
