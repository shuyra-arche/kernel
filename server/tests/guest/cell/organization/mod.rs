mod flat;
mod nest;
pub(super) fn run(at: usize, count: usize) {
    use kernel_api::boot::{
        args,
        ledger::{Call, Entries, Name},
    };
    use kernel_api::model::unit::ControlId;
    let words = unsafe { core::slice::from_raw_parts(at as *const usize, count) };
    let bytes = unsafe {
        core::slice::from_raw_parts(words[args::LEDGER] as *const u8, words[args::LEDGER_LEN])
    };
    let build = Entries::new(bytes)
        .unwrap()
        .find(Name::Call(Call::Build))
        .unwrap();
    flat::run();
    nest::run();
    let owner = crate::lifecycle::spawn(nest::cascade as *const () as usize);
    crate::license::delegate(owner);
    kernel_client::call::unit::accord(ControlId::new(build.handle()), owner, false).unwrap();
    kernel_client::call::unit::resume(kernel_api::model::unit::UnitTarget::Task(owner)).unwrap();
    assert!(crate::lifecycle::wait_task(owner));
    assert_eq!(crate::lifecycle::result(owner), (1, 84));
    crate::lifecycle::delete(owner);
    use core::fmt::Write;
    let _ = writeln!(
        crate::Console,
        "PASS organization flat=1 window=1 transfer=1 share=1 revoke=1 internal-nest=1 retirement=8 cascade=8"
    );
}
