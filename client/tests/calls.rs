use std::cell::RefCell;

use env::{
    KernelCall, MemoryFail, Operation, PieToken, RegisterCall, TaskId, TeamId, ViewId,
    ViewSelection, Wait,
};
use kernel_client::invoke::invoke_with;

#[macro_use]
#[path = "../src/call/macros.rs"]
mod templates;

#[derive(Default)]
struct Transport {
    reply: (usize, usize, usize),
    calls: Vec<(usize, [usize; 6])>,
}
thread_local! {
    static TRANSPORT: RefCell<Transport> = RefCell::new(Transport::default());
}

unsafe fn invoke_call<C: KernelCall>(call: C) -> Result<C::Output, C::Error> {
    invoke_with(call, |slot, arguments| {
        TRANSPORT.with_borrow_mut(|transport| {
            transport.calls.push((slot, arguments));
            transport.reply
        })
    })
}

#[allow(dead_code)]
mod named {
    use super::invoke_call;
    include!("../src/call/definitions.rs");
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
    assert_eq!(named::chrono::clock(), u64::MAX);
    called((4usize << 32) | 1, [0; 6]);
    reply((usize::MAX, 0, 0));
    assert!(named::room::wake(19));
    called(4, [19, 0, 0, 0, 0, 0]);
    reply((0, 0, 0));
    named::room::starve();
    called(0, [0; 6]);
}

#[test]
fn failures_preserve_the_domain_and_do_not_retry() {
    reply(((-7isize) as usize, 0, 0));
    let reference = env::FlatReference::from_bytes(&17u64.to_le_bytes()).unwrap();
    let error = named::memory::release(reference).unwrap_err();
    assert_eq!(error.source, MemoryFail::Busy);
    called((2usize << 32) | 6, [17, 0, 0, 0, 0, 0]);
}

#[test]
fn structured_arguments_keep_all_six_registers_and_the_explicit_slot() {
    reply((0, 0, 0));
    let view = ViewId::from_bytes(&23u64.to_le_bytes()).unwrap();
    named::memory::set_access(
        TeamId::new(17),
        view,
        ViewSelection::Pages {
            offset: 3,
            count: 5,
        },
        6,
    )
    .unwrap();
    called((2usize << 32) | 9, [17, 23, 1, 3, 5, 6]);
    let decoded = env::EnvCall::from_wire((2usize << 32) | 9, &[17, 23, 1, 3, 5, 6]).unwrap();
    assert!(
        matches!(decoded, env::EnvCall::Memory(env::MemoryCall::SetAccess {
        team, view: decoded_view, selection: ViewSelection::Pages { offset: 3, count: 5 }, access: 6,
    }) if team == TeamId::new(17) && decoded_view == view)
    );
}

#[test]
fn three_word_returns_and_keyword_named_calls_are_preserved() {
    reply((7, 23, 91));
    let token = PieToken::mint(17);
    assert_eq!(named::hole::peek(token).unwrap(), (7, TaskId::new(23), 91));
    called((5usize << 32) | 4, [17, 0, 0, 0, 0, 0]);

    let expected = env::ToleAwaitReply::Source {
        source: env::ToleEventSource::Inspect {
            task: TaskId::new(42),
            token,
        },
        fail: Some(env::ToleEventFail::Busy),
    };
    let words = expected.words();
    reply((words[0], words[1], words[2]));
    assert_eq!(named::tole::await_(token, Wait::Forever).unwrap(), expected);
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
        fn from_registers(_: &[usize; 6]) -> Result<Self, env::Decode> {
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
    use env::abi::call::{chrono::operations::Clock, memory::operations::Release};
    let reference = env::FlatReference::from_bytes(&17u64.to_le_bytes()).unwrap();
    let released: Result<(), erra::Error<MemoryFail>> =
        invoke_with(Release { reference }, |_, _| (0, 0, 0));
    assert_eq!(released.unwrap(), ());
    let clock: Result<u64, core::convert::Infallible> =
        invoke_with(Clock, |_, _| (usize::MAX, 0, 0));
    assert_eq!(clock, Ok(u64::MAX));
}
