fn main() {
    let ld = std::env::var("DEP_KERNEL_STD_RT_LINKER_SCRIPT")
        .expect("kernel-std-rt linker metadata missing");
    println!("cargo::rustc-link-arg=-T{ld}");
    println!("cargo::rerun-if-env-changed=DEP_KERNEL_STD_RT_LINKER_SCRIPT");
}
