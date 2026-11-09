fn main() {
    let script = std::env::var("DEP_SHUYRA_KERNEL_LINKER_SCRIPT").unwrap();
    println!("cargo::rustc-link-arg=--defsym=KERNEL_LOAD_ADDRESS=0x80200000");
    println!("cargo::rustc-link-arg=-T{script}");
    println!("cargo::rerun-if-changed={script}");
}
