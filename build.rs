fn main() {
    println!("cargo:rerun-if-changed=assets/facelock.rc");
    println!("cargo:rerun-if-changed=assets/facelock.manifest");
    embed_resource::compile("assets/facelock.rc", embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
