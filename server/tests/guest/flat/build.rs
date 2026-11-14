fn main() {
    assert!(
        std::env::var_os("FLAT_PROBE_PROVIDER").is_some(),
        "compose this guest through server/tests/guest/flat/run.py"
    );
    let script = std::env::var("DEP_SHUYRA_KERNEL_LINKER_SCRIPT").unwrap();
    println!("cargo::rustc-link-arg=--defsym=KERNEL_LOAD_ADDRESS=0x80200000");
    println!("cargo::rustc-link-arg=-T{script}");
    println!("cargo::rerun-if-changed={script}");
}
