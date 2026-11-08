use kernel_api::abi::operation::KernelCall;
use kernel_api::abi::operation::RegisterCall;
use kernel_api::abi::operation::TerminalCall;

#[test]
fn domain_decoders_reject_foreign_classes_and_unknown_slots() {
    // The operation number alone is valid for Chrono; the class is not.
    assert_eq!(
        kernel_api::abi::call::sign::SignCall::from_wire(1, &[0; 6]),
        Err(kernel_api::abi::register::Decode::BadSlot)
    );
    assert_eq!(
        kernel_api::abi::call::sign::SignCall::from_wire((4usize << 32) | 99, &[0; 6]),
        Err(kernel_api::abi::register::Decode::BadSlot)
    );
    assert_eq!(
        kernel_api::abi::call::EnvCall::from_wire(3usize << 32, &[0; 6]),
        Err(kernel_api::abi::register::Decode::BadSlot)
    );
}

#[test]
fn typed_and_dispatch_requests_share_encoding_and_validation() {
    use kernel_api::abi::call::room::operations::WaitValue;
    let words = [0x4000, 23, 1, 1234, 0, 0];
    let request = WaitValue::from_registers(&words).unwrap();
    assert_eq!(request.pack(), words);
    let aggregate: kernel_api::abi::call::room::RoomCall = request.into();
    assert_eq!(
        kernel_api::abi::call::room::RoomCall::from_wire(WaitValue::SLOT, &words).unwrap(),
        aggregate
    );
    let malformed = [0x4000, 23, 3, 1234, 0, 0];
    assert!(WaitValue::from_registers(&malformed).is_err());
    assert!(kernel_api::abi::call::room::RoomCall::from_wire(WaitValue::SLOT, &malformed).is_err());
}

#[test]
fn terminal_operations_keep_their_request_abi_without_a_return_decoder() {
    use kernel_api::abi::call::unit::operations::Exit;
    fn terminal<C: TerminalCall>() {}
    terminal::<Exit>();
    let request = Exit {
        value: 17,
        reason: 2,
        note: kernel_api::address::VirtAddr::new(0x1234),
        len: 8,
    };
    assert_eq!(Exit::SLOT, kernel_api::abi::PREFIX | (1usize << 32) | 5);
    assert_eq!(request.pack(), [17, 2, 0x1234, 8, 0, 0]);
    assert!(matches!(
        kernel_api::abi::call::unit::UnitCall::from_wire(Exit::SLOT, &request.pack()),
        Ok(kernel_api::abi::call::unit::UnitCall::Exit {
            reason: 2,
            len: 8,
            ..
        })
    ));
}

#[test]
fn unknown_failure_codes_remain_abi_violations() {
    use kernel_api::abi::call::cell::operations::Delete;
    assert!(
        std::panic::catch_unwind(|| Delete::decode_return(((-100isize) as usize, 0, 0))).is_err()
    );
}

#[test]
fn error_domains_preserve_busy_and_auxiliary_codes() {
    assert_eq!(kernel_api::abi::call::cell::CellFail::Busy.code(), -7);
    assert!(
        kernel_api::abi::call::cell::CellFail::of_code(-7)
            .unwrap()
            .is_busy()
    );
    assert_eq!(kernel_api::abi::call::sign::SignEventFail::Busy.code(), -3);
    assert!(
        kernel_api::abi::call::sign::SignEventFail::of_code(-3)
            .unwrap()
            .is_busy()
    );
    assert_eq!(kernel_api::abi::error::DispatchFail::Unknown.code(), -1);
    assert!(kernel_api::abi::error::DispatchFail::of_code(0).is_none());
}

#[test]
fn out_of_memory_keeps_each_domain_code() {
    use kernel_api::abi::call::{
        cell::CellFail,
        clone_view::CloneFail,
        pend::PendFail,
        room::RoomFail,
        share_view::ShareFail,
        sign::{SignEventFail, SignFail, SignReply},
        unit::UnitFail,
    };
    use kernel_api::model::{
        identity::{PendId, TaskId},
        sign::SignSource,
    };

    assert_eq!(
        [
            CellFail::OutOfMemory.code(),
            CloneFail::OutOfMemory.code(),
            PendFail::OutOfMemory.code(),
            RoomFail::OutOfMemory.code(),
            ShareFail::OutOfMemory.code(),
            SignFail::OutOfMemory.code(),
            SignEventFail::OutOfMemory.code(),
            UnitFail::OutOfMemory.code(),
        ],
        [-2, -4, -3, -2, -4, -4, -4, -3],
    );
    let reply = SignReply::Source {
        source: SignSource::Inspect {
            task: TaskId::new(17),
            token: PendId::mint(23),
        },
        fail: Some(SignEventFail::OutOfMemory),
    };
    let words = reply.words();
    assert_eq!(words[0] >> 8, 4);
    assert_eq!(SignReply::of(words), Some(reply));
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
    let token = kernel_api::model::identity::PendId::mint(17);
    let team = kernel_api::model::identity::TeamId::new(19);
    let reference = kernel_api::model::cell::SlotId::from_bytes(&29u64.to_le_bytes()).unwrap();
    let range = kernel_api::model::cell::ReferenceRange::new(reference, 3, 5).unwrap();
    let allocation = kernel_api::model::cell::Allocation::new(4096, 4096).unwrap();
    check(allocation);
    check(range);
    check(kernel_api::model::pend::PendRange::Inherit);
    check(kernel_api::model::pend::PendRange::pages(3, 5).unwrap());
    check(kernel_api::model::unit::UnitTarget::Task(
        kernel_api::model::identity::TaskId::new(31),
    ));
    check(kernel_api::model::unit::UnitTarget::Team(team));
    check(kernel_api::model::clone_view::CloneLimits {
        max_len: 64,
        max_messages: 2,
        max_bytes: 128,
    });
    check(kernel_api::model::sign::SignSource::Inspect {
        task: kernel_api::model::identity::TaskId::new(31),
        token,
    });
}

#[test]
fn external_mapping_requires_a_target_pend_and_retired_device_abi_is_rejected() {
    use kernel_api::{
        abi::call::{EnvCall, cell::operations::Create},
        model::{
            cell::{MappingId, MappingTarget},
            identity::{PendId, TeamId},
            sign::SignSource,
        },
    };
    assert_eq!(Create::SLOT, kernel_api::abi::PREFIX | (2usize << 32));
    for slot in 0..10 {
        assert!(EnvCall::from_wire((13usize << 32) | slot, &[1; 6]).is_err());
    }
    for slot in 8..20 {
        assert!(
            EnvCall::from_wire(kernel_api::abi::PREFIX | (2usize << 32) | slot, &[1; 6]).is_err()
        );
    }
    let token = PendId::mint(17);
    let mapping = MappingId::from_bytes(&23u64.to_le_bytes()).unwrap();
    for source in [
        SignSource::MappingRetired { token, mapping },
        SignSource::MappingFault { token, mapping },
    ] {
        assert_eq!(SignSource::of(source.words()), Some(source));
        let mut words = source.words();
        words[2] = 0;
        assert_eq!(SignSource::of(words), None);
    }
    assert!(SignSource::of([7, 1, 0]).is_none());
    assert!(SignSource::of([8, 1, 0]).is_none());
    use kernel_api::abi::register::Wire;
    for target in [MappingTarget::Cpu(TeamId::new(0)), MappingTarget::Io(token)] {
        let mut words = [0; 6];
        target.pack(&mut words, &mut 0);
        assert_eq!(MappingTarget::unpack(&words, &mut 0).unwrap(), target);
    }
}

#[test]
fn sign_rights_reject_unknown_and_empty_authority_without_truncation() {
    use kernel_api::{abi::register::Wire, model::sign::SignRights};
    for bits in [0, 32, 256, usize::MAX] {
        assert!(SignRights::unpack(&[bits, 0, 0, 0, 0, 0], &mut 0).is_err());
    }
    for bits in 1..32 {
        assert_eq!(
            SignRights::unpack(&[bits, 0, 0, 0, 0, 0], &mut 0)
                .unwrap()
                .bits() as usize,
            bits
        );
    }
    assert!(!SignRights::LISTEN.contains(SignRights::TRIGGER));
    assert!(!SignRights::TRIGGER.contains(SignRights::ROUTE));
    assert!(kernel_api::abi::call::EnvCall::from_wire(12usize << 32, &[0; 6]).is_err());
    use kernel_api::abi::reply::SignReply;
    assert_eq!(
        SignReply::of(SignReply::Triggered { sequence: 1 }.words()),
        Some(SignReply::Triggered { sequence: 1 })
    );
}

#[test]
fn data_authority_uses_one_bit_and_retired_signal_slots_are_invalid() {
    use kernel_api::{
        abi::{call::EnvCall, register::Wire},
        model::modifiable::Modifiable,
    };
    for raw in [2, 4, 8, 16, 0x1_0000_0001usize, usize::MAX] {
        assert!(Modifiable::unpack(&[raw, 0, 0, 0, 0, 0], &mut 0).is_err());
    }
    for (raw, expected) in [(0, Modifiable::No), (1, Modifiable::Yes)] {
        assert_eq!(
            Modifiable::unpack(&[raw, 0, 0, 0, 0, 0], &mut 0).unwrap(),
            expected
        );
    }
    for (class, slot) in [(5, 6), (5, 7), (10, 4), (10, 5), (10, 6)] {
        assert!(EnvCall::from_wire((class << 32) | slot, &[0; 6]).is_err());
    }
    use kernel_api::model::pend::PendInfo;
    let active = [1, 1, 2, 3, 4, 5, 0, 1, 7, 1, 1, 0, 0, 0];
    assert_eq!(PendInfo::from_words(active).unwrap().words(), active);
    for (index, value) in [(0, 0), (1, 0), (2, 0), (9, 2), (6, 4)] {
        let mut bad = active;
        bad[index] = value;
        assert!(PendInfo::from_words(bad).is_none());
    }
}

#[test]
fn clock_scale_never_rounds_a_deadline_down_or_wraps() {
    use core::num::NonZeroU64;
    use kernel_api::model::clock::ClockSource;

    let clock = ClockSource::new(13, NonZeroU64::new(3_000_000).unwrap());
    assert_eq!(clock.nanos(12), 0);
    assert_eq!(clock.deadline(334), 15);
    assert!(clock.nanos(14) < 334);
    assert!(clock.nanos(15) >= 334);
    assert_eq!(clock.deadline(0), 13);
    let edge = ClockSource::new(u64::MAX - 1, NonZeroU64::new(u64::MAX).unwrap());
    assert_eq!(edge.duration_ticks(u128::MAX), u64::MAX);
    assert_eq!(edge.deadline(u64::MAX), u64::MAX);
    let slow = ClockSource::new(0, NonZeroU64::MIN);
    assert_eq!(slow.nanos(u64::MAX), u64::MAX);
}
