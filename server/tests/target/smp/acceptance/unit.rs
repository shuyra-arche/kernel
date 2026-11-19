//! External Tasks are User-only, including the boot Capsule with Build.
use super::*;
use core::fmt::Write;
use kernel_client::call::unit;

pub fn run() {
    // An external Task must fault on privileged CSR access, even though its
    // parent has Build. Observe execution failure through the public receipt.
    let probe = spawn(supervisor_csr);
    unit::resume(UnitTarget::Task(probe)).unwrap();
    assert!(matches!(join(probe).exit, Some((ExitCause::Fault, _))));
    let before = children();
    let mut bytes = UnitCreate::Team {
        guard: ControlId::NONE,
        nest: None,
        member_exit: true,
    }
    .to_bytes();
    for retired in [1u64, 2, 255, u64::MAX] {
        bytes[8..16].copy_from_slice(&retired.to_le_bytes());
        assert_eq!(
            unit::create(VirtAddr::new(bytes.as_ptr() as usize))
                .unwrap_err()
                .into_source(),
            kernel_api::abi::call::unit::UnitFail::Denied
        );
        assert_eq!(
            children(),
            before,
            "invalid descriptor must not publish a Team"
        );
    }
    let task = spawn(unprivileged_builder);
    unit::resume(UnitTarget::Task(task)).unwrap();
    assert_eq!(join(task).result, Some(82));
    let _ = writeln!(
        Console,
        "PASS acceptance-unit user-caller=1 privileged-csr=fault retired-kind=reject unknown-kind=reject no-build=denied no-child-published=1"
    );
}
fn children() -> usize {
    let mut count = 0;
    let mut after = TeamId::new(0);
    loop {
        let mut bytes = [0; UnitInfo::WIDTH];
        if !unit::inspect(
            UnitQuery::Children { after },
            VirtAddr::new(bytes.as_mut_ptr() as usize),
        )
        .unwrap()
        {
            return count;
        }
        let info = UnitInfo::from_bytes(&bytes).unwrap();
        let UnitTarget::Team(team) = info.target else {
            panic!("Team")
        };
        after = team;
        count += 1;
    }
}
extern "C" fn unprivileged_builder() -> ! {
    let bytes = UnitCreate::Team {
        guard: ControlId::NONE,
        nest: None,
        member_exit: true,
    }
    .to_bytes();
    assert_eq!(
        unit::create(VirtAddr::new(bytes.as_ptr() as usize))
            .unwrap_err()
            .into_source(),
        kernel_api::abi::call::unit::UnitFail::Denied
    );
    unit::exit(82, 0, VirtAddr::new(0), 0, ExitScope::Task)
}

extern "C" fn supervisor_csr() -> ! {
    unsafe { core::arch::asm!("csrr {0}, sstatus", out(reg) _, options(nostack)); }
    // Reaching this exit means the privilege boundary failed.
    unit::exit(0, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
