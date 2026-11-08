fn main() {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("link.x");
    println!("cargo::rustc-link-arg-bin=shuyra=-T{}", script.display());
    println!("cargo::rerun-if-changed=link.x");
    println!("cargo::metadata=linker_script={}", script.display());
}
