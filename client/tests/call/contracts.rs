use std::cell::RefCell;

use kernel_api::abi::call::cell::CellFail;
use kernel_api::abi::operation::KernelCall;
use kernel_api::abi::operation::Operation;
use kernel_api::abi::operation::RegisterCall;
use kernel_api::model::identity::PendId;
use kernel_api::model::identity::TaskId;
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
    TRANSPORT.with_borrow(|transport| {
        assert_eq!(
            transport.calls,
            vec![(kernel_api::abi::PREFIX | slot, arguments)]
        )
    });
}

#[test]
fn infallible_returns_keep_the_high_bit_even_in_a_fallible_domain() {
    reply((usize::MAX, 0, 0));
    assert_eq!(
        invoke(kernel_api::abi::call::room::operations::Clock).unwrap(),
        u64::MAX
    );
    called(4, [0; 6]);
    reply((0, 0, 0));
    invoke(kernel_api::abi::call::room::operations::Yield).unwrap();
    called(0, [0; 6]);
}

#[test]
fn failures_preserve_the_domain_and_do_not_retry() {
    reply(((-7isize) as usize, 0, 0));
    let object = kernel_api::address::VirtAddr::new(17);
    let error = invoke(kernel_api::abi::call::cell::operations::Delete { object }).unwrap_err();
    assert_eq!(error.source, CellFail::Busy);
    called((2usize << 32) | 1, [17, 0, 0, 0, 0, 0]);
}

#[test]
fn descriptor_arguments_keep_the_explicit_slot() {
    use kernel_api::address::VirtAddr;
    reply((0, 0, 0));
    invoke(kernel_api::abi::call::cell::operations::Protect {
        position: VirtAddr::new(0x4000),
        access: 6,
    })
    .unwrap();
    called((2usize << 32) | 6, [0x4000, 6, 0, 0, 0, 0]);
}

#[test]
fn three_word_returns_are_preserved() {
    reply((7, 23, 91));
    let token = PendId::mint(17);
    assert_eq!(
        invoke(kernel_api::abi::call::clone_view::operations::Peek { token }).unwrap(),
        (7, TaskId::new(23), 91)
    );
    called((5usize << 32) | 4, [17, 0, 0, 0, 0, 0]);

    let expected = kernel_api::abi::reply::SignReply::Source {
        source: kernel_api::model::sign::SignSource::Inspect {
            task: TaskId::new(42),
            token,
        },
        fail: Some(kernel_api::abi::call::sign::SignEventFail::Busy),
    };
    let words = expected.words();
    reply((words[0], words[1], words[2]));
    assert_eq!(
        invoke(kernel_api::abi::call::sign::operations::Listen {
            token: token,
            wait: Wait::Infinite
        })
        .unwrap(),
        expected
    );
    called((11usize << 32) | 2, [17, 2, 0, 0, 0, 0]);
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
    use kernel_api::abi::call::{cell::operations::Delete, room::operations::Clock};
    let object = kernel_api::address::VirtAddr::new(17);
    let released: Result<(), erra::Error<CellFail>> =
        invoke_with(Delete { object }, |_, _| (0, 0, 0));
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
    use kernel_api::address::VirtAddr;
    reply((1, 0, 0));
    assert!(
        invoke(operations::WaitValue {
            address: VirtAddr::new(0x4000),
            expected: u32::MAX,
            wait: Wait::Infinite,
        })
        .unwrap()
    );
    called(2, [0x4000, u32::MAX as usize, 2, 0, 0, 0]);
    assert!(
        matches!(EnvCall::from_wire(kernel_api::abi::PREFIX | 2, &[0x4000, 42, 0, 0, 0, 0]).unwrap(),
        EnvCall::Room(RoomCall::WaitValue { address, expected: 42, wait: Wait::Poll })
            if address == VirtAddr::new(0x4000))
    );
    assert!(
        EnvCall::from_wire(
            kernel_api::abi::PREFIX | 2,
            &[0x4000, (u32::MAX as usize) + 1, 0, 0, 0, 0]
        )
        .is_err()
    );
    assert!(EnvCall::from_wire(3, &[0x4000, 2, 0, 0, 0, 0]).is_err());
    reply((0, 0, 0));
    assert!(
        !invoke(operations::WaitValue {
            address: VirtAddr::new(0x4000),
            expected: 42,
            wait: Wait::Poll,
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
    called(3, [0x4000, 1, 0, 0, 0, 0]);
    reply(((-5isize) as usize, 0, 0));
    assert_eq!(
        invoke(operations::WaitValue {
            address: VirtAddr::new(1),
            expected: 0,
            wait: Wait::Poll,
        })
        .unwrap_err()
        .source,
        RoomFail::Invalid
    );
}

#[test]
fn clone_delivery_returns_authority_and_retires_copy_out_slot() {
    use kernel_api::abi::call::{EnvCall, clone_view::operations::Pull};
    reply((91, 5000, 23));
    let received = invoke(Pull {
        token: PendId::mint(17),
        max_len: 8192,
    })
    .unwrap();
    assert_eq!(received.token, PendId::mint(91));
    assert_eq!(received.len, 5000);
    assert_eq!(received.sender, TaskId::new(23));
    called((5usize << 32) | 8, [17, 8192, 0, 0, 0, 0]);
    assert!(EnvCall::from_wire((5usize << 32) | 3, &[17, 0x4000, 8192, 0, 0, 0]).is_err());
}

#[test]
fn cell_inspection_uses_an_output_snapshot_and_create_returns_identity() {
    use kernel_api::{
        abi::call::cell::operations::{Create, Inspect},
        address::VirtAddr,
    };
    reply((0, 0, 0));
    invoke(Inspect {
        position: VirtAddr::new(0x4000),
        out: VirtAddr::new(0x5000),
    })
    .unwrap();
    called((2usize << 32) | 7, [0x4000, 0x5000, 0, 0, 0, 0]);
    reply((1, 23, 0x6000));
    let made = invoke(Create {
        license: kernel_api::model::identity::PendId::mint(9),
        request: VirtAddr::new(0x4000),
    })
    .unwrap();
    assert_eq!(
        made,
        kernel_api::model::cell::CreatedCell::decode(1, 23, 0x6000).unwrap()
    );
    called(2usize << 32, [9, 0x4000, 0, 0, 0, 0]);
}

#[test]
fn pend_accord_returns_the_new_identity_and_return_preserves_busy() {
    use kernel_api::{
        abi::call::pend::{
            PendFail,
            operations::{Accord, Return},
        },
        address::VirtAddr,
    };
    reply((91, 0, 0));
    assert_eq!(
        invoke(Accord {
            request: VirtAddr::new(0x4000)
        })
        .unwrap(),
        PendId::mint(91)
    );
    called((7usize << 32) | 14, [0x4000, 0, 0, 0, 0, 0]);
    reply(((-6isize) as usize, 0, 0));
    assert_eq!(
        invoke(Return {
            token: PendId::mint(91)
        })
        .unwrap_err()
        .source,
        PendFail::Busy
    );
    called((7usize << 32) | 15, [91, 0, 0, 0, 0, 0]);
}
