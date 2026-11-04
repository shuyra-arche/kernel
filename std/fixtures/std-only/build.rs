fn main() {
    let ld = format!("{}/link.ld", env!("CARGO_MANIFEST_DIR"));
    println!("cargo::rustc-link-arg=-T{ld}");
    println!("cargo::rerun-if-changed=link.ld");
}
