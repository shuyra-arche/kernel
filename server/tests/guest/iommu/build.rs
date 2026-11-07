fn main() {
    let root = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg=-T{root}/link.ld");
    println!("cargo:rerun-if-changed=link.ld");
}
