mod flat;
mod nest;
pub(super) fn run() {
    flat::run();
    nest::run();
    use core::fmt::Write;
    let _ = writeln!(
        crate::Console,
        "PASS organization flat=1 window=1 transfer=1 share=1 revoke=1 internal-nest=1"
    );
}
