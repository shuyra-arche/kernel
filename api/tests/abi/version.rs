use kernel_api::{
    abi::{
        self,
        call::{EnvCall, room::operations::SleepUntil},
        operation::RegisterCall,
        register::Wire,
    },
    model::wait::Wait,
};
#[test]
fn old_and_future_versions_cannot_execute_reused_operation_numbers() {
    assert_eq!(abi::VERSION, 10);
    let call = SleepUntil { at: u64::MAX };
    assert!(EnvCall::from_wire(SleepUntil::SLOT, &call.pack()).is_ok());
    for version in (0..abi::VERSION).chain([abi::VERSION + 1, u16::MAX]) {
        let slot = (SleepUntil::SLOT & ((1usize << 48) - 1)) | ((version as usize) << 48);
        assert!(EnvCall::from_wire(slot, &call.pack()).is_err());
    }
    for domain in [4usize, 6, 15, 16] {
        assert!(EnvCall::from_wire(abi::PREFIX | (domain << 32), &[0; 6]).is_err());
    }
}
#[test]
fn waits_preserve_full_width_deadlines_and_reject_noncanonical_tags() {
    for wait in [Wait::Poll, Wait::Infinite, Wait::At(0), Wait::At(u64::MAX)] {
        let mut words = [0; 6];
        let mut cursor = 0;
        wait.pack(&mut words, &mut cursor);
        assert_eq!(cursor, 2);
        assert_eq!(Wait::unpack(&words, &mut 0).unwrap(), wait);
    }
    for (tag, value) in [(0, 1), (2, 1), (3, 0), (usize::MAX, 0)] {
        assert!(Wait::unpack(&[tag, value, 0, 0, 0, 0], &mut 0).is_err());
    }
}
