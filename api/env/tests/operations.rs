use env::{KernelCall, RegisterCall, TerminalCall};
use kernel_api_env as env;

#[test]
fn domain_decoders_reject_foreign_classes_and_unknown_slots() {
    // The operation number alone is valid for Chrono; the class is not.
    assert_eq!(
        env::ChronoCall::from_wire(1, &[0; 6]),
        Err(env::Decode::BadSlot)
    );
    assert_eq!(
        env::ChronoCall::from_wire((4usize << 32) | 99, &[0; 6]),
        Err(env::Decode::BadSlot)
    );
    assert_eq!(
        env::EnvCall::from_wire(3usize << 32, &[0; 6]),
        Err(env::Decode::BadSlot)
    );
}

#[test]
fn typed_and_dispatch_requests_share_encoding_and_validation() {
    use env::abi::call::memory::operations::SetAccess;
    let words = [17, 23, 1, 3, 5, 6];
    let request = SetAccess::from_registers(&words).unwrap();
    assert_eq!(request.pack(), words);
    let aggregate: env::MemoryCall = request.into();
    assert_eq!(
        env::MemoryCall::from_wire(SetAccess::SLOT, &words).unwrap(),
        aggregate
    );
    let malformed = [17, 23, 9, 3, 5, 6];
    assert_eq!(
        SetAccess::from_registers(&malformed),
        Err(env::Decode::Invalid)
    );
    assert_eq!(
        env::MemoryCall::from_wire(SetAccess::SLOT, &malformed),
        Err(env::Decode::Invalid)
    );
}

#[test]
fn terminal_operations_keep_their_request_abi_without_a_return_decoder() {
    use env::abi::call::room::operations::Reap;
    fn terminal<C: TerminalCall>() {}
    terminal::<Reap>();
    let request = Reap {
        reason: 2,
        note: env::VirtAddr::new(0x1234),
        len: 8,
    };
    assert_eq!(Reap::SLOT, 2);
    assert_eq!(request.pack(), [2, 0x1234, 8, 0, 0, 0]);
    assert!(matches!(
        env::RoomCall::from_wire(Reap::SLOT, &request.pack()),
        Ok(env::RoomCall::Reap {
            reason: 2,
            len: 8,
            ..
        })
    ));
}

#[test]
fn unknown_failure_codes_remain_abi_violations() {
    use env::abi::call::memory::operations::Release;
    assert!(
        std::panic::catch_unwind(|| Release::decode_return(((-100isize) as usize, 0, 0))).is_err()
    );
}

#[test]
fn error_domains_preserve_busy_and_auxiliary_codes() {
    assert_eq!(env::MemoryFail::Busy.code(), -7);
    assert!(env::MemoryFail::of_code(-7).unwrap().is_busy());
    assert_eq!(env::ToleEventFail::Busy.code(), -3);
    assert!(env::ToleEventFail::of_code(-3).unwrap().is_busy());
    assert_eq!(env::DispatchFail::Unknown.code(), -1);
    assert!(env::DispatchFail::of_code(0).is_none());
}

#[test]
fn compound_field_widths_match_their_actual_register_consumption() {
    fn check<T: env::Wire + PartialEq + core::fmt::Debug>(value: T) {
        let mut words = [0usize; 6];
        let mut packed = 0;
        value.pack(&mut words, &mut packed);
        assert_eq!(packed, T::WORDS);
        let mut unpacked = 0;
        assert_eq!(T::unpack(&words, &mut unpacked).unwrap(), value);
        assert_eq!(unpacked, T::WORDS);
    }
    let token = env::PieToken::mint(17);
    let team = env::TeamId::new(19);
    let view = env::ViewId::from_bytes(&23u64.to_le_bytes()).unwrap();
    let reference = env::FlatReference::from_bytes(&29u64.to_le_bytes()).unwrap();
    let range = env::ReferenceRange::new(reference, 3, 5).unwrap();
    let allocation = env::Allocation::new(4096, 4096).unwrap();
    check(allocation);
    check(range);
    check(env::AcquireSource::Allocation(allocation));
    check(env::AcquireSource::Reference(range));
    check(env::AcquireSource::Source {
        token,
        offset: 3,
        count: 5,
    });
    check(env::AcquireSource::View {
        team,
        view,
        offset: 3,
        count: 5,
    });
    check(env::GrantRange::Inherit);
    check(env::GrantRange::pages(3, 5).unwrap());
    check(env::ViewSelection::Whole);
    check(env::ViewSelection::pages(3, 5).unwrap());
    check(env::UnitTarget::Task(env::TaskId::new(31)));
    check(env::UnitTarget::Team(team));
    check(env::HoleLimits {
        max_len: 64,
        max_messages: 2,
        max_bytes: 128,
    });
    check(env::ToleEventSource::Inspect {
        task: env::TaskId::new(31),
        token,
    });
}
