use kernel_api::abi::operation::{KernelCall, RegisterCall, TerminalCall};

#[test]
fn domain_decoders_reject_foreign_classes_and_unknown_slots() {
    // The operation number alone is valid for Chrono; the class is not.
    assert_eq!(
        kernel_api::abi::call::chrono::ChronoCall::from_wire(1, &[0; 6]),
        Err(kernel_api::abi::register::Decode::BadSlot)
    );
    assert_eq!(
        kernel_api::abi::call::chrono::ChronoCall::from_wire((4usize << 32) | 99, &[0; 6]),
        Err(kernel_api::abi::register::Decode::BadSlot)
    );
    assert_eq!(
        kernel_api::abi::call::EnvCall::from_wire(3usize << 32, &[0; 6]),
        Err(kernel_api::abi::register::Decode::BadSlot)
    );
}

#[test]
fn typed_and_dispatch_requests_share_encoding_and_validation() {
    use kernel_api::abi::call::memory::operations::SetAccess;
    let words = [17, 23, 1, 3, 5, 6];
    let request = SetAccess::from_registers(&words).unwrap();
    assert_eq!(request.pack(), words);
    let aggregate: kernel_api::abi::call::memory::MemoryCall = request.into();
    assert_eq!(
        kernel_api::abi::call::memory::MemoryCall::from_wire(SetAccess::SLOT, &words).unwrap(),
        aggregate
    );
    let malformed = [17, 23, 9, 3, 5, 6];
    assert_eq!(
        SetAccess::from_registers(&malformed),
        Err(kernel_api::abi::register::Decode::Invalid)
    );
    assert_eq!(
        kernel_api::abi::call::memory::MemoryCall::from_wire(SetAccess::SLOT, &malformed),
        Err(kernel_api::abi::register::Decode::Invalid)
    );
}

#[test]
fn terminal_operations_keep_their_request_abi_without_a_return_decoder() {
    use kernel_api::abi::call::room::operations::Reap;
    fn terminal<C: TerminalCall>() {}
    terminal::<Reap>();
    let request = Reap {
        reason: 2,
        note: kernel_api::model::identity::VirtAddr::new(0x1234),
        len: 8,
    };
    assert_eq!(Reap::SLOT, 2);
    assert_eq!(request.pack(), [2, 0x1234, 8, 0, 0, 0]);
    assert!(matches!(
        kernel_api::abi::call::room::RoomCall::from_wire(Reap::SLOT, &request.pack()),
        Ok(kernel_api::abi::call::room::RoomCall::Reap {
            reason: 2,
            len: 8,
            ..
        })
    ));
}

#[test]
fn unknown_failure_codes_remain_abi_violations() {
    use kernel_api::abi::call::memory::operations::Release;
    assert!(
        std::panic::catch_unwind(|| Release::decode_return(((-100isize) as usize, 0, 0))).is_err()
    );
}

#[test]
fn error_domains_preserve_busy_and_auxiliary_codes() {
    assert_eq!(kernel_api::abi::call::memory::MemoryFail::Busy.code(), -7);
    assert!(
        kernel_api::abi::call::memory::MemoryFail::of_code(-7)
            .unwrap()
            .is_busy()
    );
    assert_eq!(kernel_api::abi::call::tole::ToleEventFail::Busy.code(), -3);
    assert!(
        kernel_api::abi::call::tole::ToleEventFail::of_code(-3)
            .unwrap()
            .is_busy()
    );
    assert_eq!(kernel_api::abi::error::DispatchFail::Unknown.code(), -1);
    assert!(kernel_api::abi::error::DispatchFail::of_code(0).is_none());
}

#[test]
fn compound_field_widths_match_their_actual_register_consumption() {
    fn check<T: kernel_api::abi::register::Wire + PartialEq + core::fmt::Debug>(value: T) {
        let mut words = [0usize; 6];
        let mut packed = 0;
        value.pack(&mut words, &mut packed);
        assert_eq!(packed, T::WORDS);
        let mut unpacked = 0;
        assert_eq!(T::unpack(&words, &mut unpacked).unwrap(), value);
        assert_eq!(unpacked, T::WORDS);
    }
    let token = kernel_api::model::identity::PieToken::mint(17);
    let team = kernel_api::model::identity::TeamId::new(19);
    let mapping = kernel_api::model::memory::MappingId::from_bytes(&23u64.to_le_bytes()).unwrap();
    let reference =
        kernel_api::model::memory::FlatReference::from_bytes(&29u64.to_le_bytes()).unwrap();
    let range = kernel_api::model::memory::ReferenceRange::new(reference, 3, 5).unwrap();
    let allocation = kernel_api::model::memory::Allocation::new(4096, 4096).unwrap();
    check(allocation);
    check(range);
    check(kernel_api::model::memory::AcquireSource::Allocation(
        allocation,
    ));
    check(kernel_api::model::memory::AcquireSource::Reference(range));
    check(kernel_api::model::memory::AcquireSource::Source {
        token,
        offset: 3,
        count: 5,
    });
    check(kernel_api::model::memory::AcquireSource::Mapping {
        team,
        mapping,
        offset: 3,
        count: 5,
    });
    check(kernel_api::model::pie::GrantRange::Inherit);
    check(kernel_api::model::pie::GrantRange::pages(3, 5).unwrap());
    check(kernel_api::model::memory::MappingSelection::Whole);
    check(kernel_api::model::memory::MappingSelection::pages(3, 5).unwrap());
    check(kernel_api::model::unit::UnitTarget::Task(
        kernel_api::model::identity::TaskId::new(31),
    ));
    check(kernel_api::model::unit::UnitTarget::Team(team));
    check(kernel_api::model::hole::HoleLimits {
        max_len: 64,
        max_messages: 2,
        max_bytes: 128,
    });
    check(kernel_api::model::tole::ToleEventSource::Inspect {
        task: kernel_api::model::identity::TaskId::new(31),
        token,
    });
}

#[test]
fn device_authority_is_not_an_address_and_event_tags_are_distinct() {
    use kernel_api::{abi::call::device::operations::Publish, model::tole::ToleEventSource};
    assert_eq!(Publish::SLOT, (13usize << 32) | 5);
    assert!(Publish::from_registers(&[1, 1, 0x400000, 2, 0, 0]).is_err());
    for source in [
        ToleEventSource::DeviceRetired { device: 1 },
        ToleEventSource::DeviceFault { device: 1 },
    ] {
        assert_eq!(ToleEventSource::of(source.words()), Some(source));
        let mut words = source.words();
        words[1] = 0;
        assert_eq!(ToleEventSource::of(words), None);
        words = source.words();
        words[2] = 1;
        assert_eq!(ToleEventSource::of(words), None);
    }
}
