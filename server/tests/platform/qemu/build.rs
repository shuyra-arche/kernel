fn main() {
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../link.x");
    println!("cargo::rustc-link-arg=-T{}", script.display());
    println!("cargo::rerun-if-changed={}", script.display());
}
