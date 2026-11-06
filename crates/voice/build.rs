use std::process::Command;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    // ggml-metal's @available calls a clang runtime symbol below macOS 15; rustc skips that runtime
    let output = Command::new("clang")
        .arg("--print-runtime-dir")
        .output()
        .expect("aether-voice needs clang to find its runtime: install Xcode command line tools");
    assert!(
        output.status.success(),
        "clang --print-runtime-dir failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let dir = String::from_utf8_lossy(&output.stdout);
    println!("cargo:rustc-link-search=native={}", dir.trim());
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
}
