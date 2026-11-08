use kernel_api::abi::operation::{KernelCall, RegisterCall, TerminalCall};

#[kernel_api_macros::envcall(class = 4, call = ClockCall)]
mod clock {
    #[operation(slot = 0, output = usize, infallible)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Ticks;
    #[operation(slot = 1, nonreturning)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Stop;
}

#[kernel_api_macros::envcall]
mod errors {
    #[error_domain]
    pub enum EventFail {
        Denied = -1,
        #[busy]
        Busy = -2,
    }
}

#[test]
fn declarations_provide_typed_calls_and_dispatch() {
    fn terminal<T: TerminalCall>() {}
    terminal::<clock::Stop>();
    assert_eq!(clock::Ticks::SLOT, kernel_api::abi::PREFIX | (4usize << 32));
    assert_eq!(clock::Ticks.pack(), [0; 6]);
    assert_eq!(
        clock::Ticks::decode_return((usize::MAX, 0, 0)),
        Ok(usize::MAX)
    );
    assert_eq!(
        clock::ClockCall::from_wire(clock::Ticks::SLOT, &[0; 6]),
        Ok(clock::ClockCall::Ticks)
    );
    assert_eq!(
        clock::Stop::SLOT,
        kernel_api::abi::PREFIX | (4usize << 32) | 1
    );
    assert_eq!(
        clock::ClockCall::from_wire(clock::Stop::SLOT, &[0; 6]),
        Ok(clock::ClockCall::Stop)
    );
}

#[test]
fn auxiliary_error_domains_need_no_fake_call_class() {
    assert_eq!(errors::EventFail::Denied.code(), -1);
    assert_eq!(
        errors::EventFail::of_code(-2),
        Some(errors::EventFail::Busy)
    );
    assert!(errors::EventFail::Busy.is_busy());
    assert!(errors::EventFail::of_code(0).is_none());
}

#[test]
fn invalid_declarations_report_compile_errors() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/declaration/fixtures/*.rs");
}
