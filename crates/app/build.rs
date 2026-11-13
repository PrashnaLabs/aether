fn main() {
    println!("cargo:rerun-if-changed=aether.rc");
    println!("cargo:rerun-if-changed=assets/app-icons/Aether.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("aether.rc", embed_resource::NONE)
            .manifest_required()
            .expect("couldn't embed the Windows app icon");
    }
}
