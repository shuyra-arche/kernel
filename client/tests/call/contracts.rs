use std::cell::RefCell;

use kernel_api::abi::call::memory::MemoryFail;
use kernel_api::abi::operation::{KernelCall, Operation, RegisterCall};
use kernel_api::model::identity::{PieToken, TaskId, TeamId};
use kernel_api::model::memory::{ViewId, ViewSelection};
use kernel_api::model::wait::Wait;
use kernel_client::invoke::invoke_with;

#[derive(Default)]
struct Transport {
    reply: (usize, usize, usize),
    calls: Vec<(usize, [usize; 6])>,
}
thread_local! {
    static TRANSPORT: RefCell<Transport> = RefCell::new(Transport::default());
}

fn invoke<C: KernelCall>(call: C) -> Result<C::Output, C::Error> {
    invoke_with(call, |slot, arguments| {
        TRANSPORT.with_borrow_mut(|transport| {
            transport.calls.push((slot, arguments));
            transport.reply
        })
    })
}

fn reply(value: (usize, usize, usize)) {
    TRANSPORT.with_borrow_mut(|transport| {
        transport.calls.clear();
        transport.reply = value;
    });
}

fn called(slot: usize, arguments: [usize; 6]) {
    TRANSPORT.with_borrow(|transport| assert_eq!(transport.calls, vec![(slot, arguments)]));
}

#[test]
fn infallible_returns_keep_the_high_bit_even_in_a_fallible_domain() {
    reply((usize::MAX, 0, 0));
    assert_eq!(
        invoke(kernel_api::abi::call::chrono::operations::Clock).unwrap(),
        u64::MAX
    );
    called((4usize << 32) | 1, [0; 6]);
    reply((usize::MAX, 0, 0));
    assert!(invoke(kernel_api::abi::call::room::operations::Wake { key: 19 }).unwrap());
    called(4, [19, 0, 0, 0, 0, 0]);
    reply((0, 0, 0));
    invoke(kernel_api::abi::call::room::operations::Starve).unwrap();
    called(0, [0; 6]);
}

#[test]
fn failures_preserve_the_domain_and_do_not_retry() {
    reply(((-7isize) as usize, 0, 0));
    let reference =
        kernel_api::model::memory::FlatReference::from_bytes(&17u64.to_le_bytes()).unwrap();
    let error =
        invoke(kernel_api::abi::call::memory::operations::Release { reference }).unwrap_err();
    assert_eq!(error.source, MemoryFail::Busy);
    called((2usize << 32) | 6, [17, 0, 0, 0, 0, 0]);
}

#[test]
fn structured_arguments_keep_all_six_registers_and_the_explicit_slot() {
    reply((0, 0, 0));
    let view = ViewId::from_bytes(&23u64.to_le_bytes()).unwrap();
    invoke(kernel_api::abi::call::memory::operations::SetAccess {
        team: TeamId::new(17),
        view,
        selection: ViewSelection::Pages {
            offset: 3,
            count: 5,
        },
        access: 6,
    })
    .unwrap();
    called((2usize << 32) | 9, [17, 23, 1, 3, 5, 6]);
    let decoded =
        kernel_api::abi::call::EnvCall::from_wire((2usize << 32) | 9, &[17, 23, 1, 3, 5, 6])
            .unwrap();
    assert!(
        matches!(decoded, kernel_api::abi::call::EnvCall::Memory(kernel_api::abi::call::memory::MemoryCall::SetAccess {
        team, view: decoded_view, selection: ViewSelection::Pages { offset: 3, count: 5 }, access: 6,
    }) if team == TeamId::new(17) && decoded_view == view)
    );
}

#[test]
fn three_word_returns_are_preserved() {
    reply((7, 23, 91));
    let token = PieToken::mint(17);
    assert_eq!(
        invoke(kernel_api::abi::call::hole::operations::Peek { token }).unwrap(),
        (7, TaskId::new(23), 91)
    );
    called((5usize << 32) | 4, [17, 0, 0, 0, 0, 0]);

    let expected = kernel_api::abi::reply::ToleAwaitReply::Source {
        source: kernel_api::model::tole::ToleEventSource::Inspect {
            task: TaskId::new(42),
            token,
        },
        fail: Some(kernel_api::abi::call::tole::ToleEventFail::Busy),
    };
    let words = expected.words();
    reply((words[0], words[1], words[2]));
    assert_eq!(
        invoke(kernel_api::abi::call::tole::operations::Await {
            tole: token,
            millis: Wait::Forever
        })
        .unwrap(),
        expected
    );
    called((12usize << 32) | 4, [17, usize::MAX, 0, 0, 0, 0]);
}

#[test]
fn generic_invocation_orders_packing_transport_and_decode_once() {
    thread_local! {
        static EVENTS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    }
    struct Probe;
    impl Operation for Probe {
        type Output = usize;
        type Error = core::convert::Infallible;
    }
    impl RegisterCall for Probe {
        const SLOT: usize = 99;
        fn pack(&self) -> [usize; 6] {
            EVENTS.with_borrow_mut(|events| events.push("pack"));
            [7; 6]
        }
        fn from_registers(_: &[usize; 6]) -> Result<Self, kernel_api::abi::register::Decode> {
            Ok(Self)
        }
    }
    impl KernelCall for Probe {
        fn decode_return(registers: (usize, usize, usize)) -> Result<usize, Self::Error> {
            EVENTS.with_borrow_mut(|events| events.push("decode"));
            Ok(registers.0 + registers.1 + registers.2)
        }
    }
    EVENTS.with_borrow_mut(Vec::clear);
    let value = invoke_with(Probe, |slot, arguments| {
        EVENTS.with_borrow_mut(|events| events.push("transport"));
        assert_eq!((slot, arguments), (99, [7; 6]));
        (1, 2, 3)
    });
    assert_eq!(value, Ok(6));
    EVENTS.with_borrow(|events| assert_eq!(*events, ["pack", "transport", "decode"]));
}

#[test]
fn operations_return_their_own_types_without_a_domain_result_enum() {
    use kernel_api::abi::call::{chrono::operations::Clock, memory::operations::Release};
    let reference =
        kernel_api::model::memory::FlatReference::from_bytes(&17u64.to_le_bytes()).unwrap();
    let released: Result<(), erra::Error<MemoryFail>> =
        invoke_with(Release { reference }, |_, _| (0, 0, 0));
    assert_eq!(released.unwrap(), ());
    let clock: Result<u64, core::convert::Infallible> =
        invoke_with(Clock, |_, _| (usize::MAX, 0, 0));
    assert_eq!(clock, Ok(u64::MAX));
}

#[test]
fn conditional_waits_encode_address_value_deadline_and_wake_scope() {
    use kernel_api::abi::call::{
        EnvCall,
        room::{RoomCall, RoomFail, operations},
    };
    use kernel_api::model::identity::VirtAddr;
    reply((1, 0, 0));
    assert!(
        invoke(operations::WaitValue {
            address: VirtAddr::new(0x4000),
            expected: u32::MAX,
            millis: Wait::Forever,
        })
        .unwrap()
    );
    called(7, [0x4000, u32::MAX as usize, usize::MAX, 0, 0, 0]);
    assert!(
        matches!(EnvCall::from_wire(7, &[0x4000, 42, 0, 0, 0, 0]).unwrap(),
        EnvCall::Room(RoomCall::WaitValue { address, expected: 42, millis: Wait::AtMost(0) })
            if address == VirtAddr::new(0x4000))
    );
    assert!(EnvCall::from_wire(7, &[0x4000, (u32::MAX as usize) + 1, 0, 0, 0, 0]).is_err());
    assert!(EnvCall::from_wire(8, &[0x4000, 2, 0, 0, 0, 0]).is_err());
    reply((0, 0, 0));
    assert!(
        !invoke(operations::WaitValue {
            address: VirtAddr::new(0x4000),
            expected: 42,
            millis: Wait::POLL,
        })
        .unwrap()
    );
    reply((3, 0, 0));
    assert_eq!(
        invoke(operations::WakeValue {
            address: VirtAddr::new(0x4000),
            all: true
        })
        .unwrap(),
        3
    );
    called(8, [0x4000, 1, 0, 0, 0, 0]);
    reply(((-5isize) as usize, 0, 0));
    assert_eq!(
        invoke(operations::WaitValue {
            address: VirtAddr::new(1),
            expected: 0,
            millis: Wait::POLL,
        })
        .unwrap_err()
        .source,
        RoomFail::Invalid
    );
}
