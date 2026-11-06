use core::sync::atomic::{AtomicU8, Ordering};

const AVAILABLE: u8 = 0;
const CLAIMED: u8 = 1;
const CONSUMED: u8 = 2;

pub(super) struct Creation {
    state: AtomicU8,
}

impl Creation {
    pub(super) fn new() -> Self {
        Self {
            state: AtomicU8::new(AVAILABLE),
        }
    }

    pub(super) fn claim(&self) -> Result<(), kernel_api::abi::call::pie::PieFail> {
        match self
            .state
            .compare_exchange(AVAILABLE, CLAIMED, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => Ok(()),
            Err(CLAIMED) => Err(kernel_api::abi::call::pie::PieFail::Busy),
            Err(_) => Err(kernel_api::abi::call::pie::PieFail::Denied),
        }
    }

    pub(super) fn consume(&self) {
        self.state.store(CONSUMED, Ordering::Release);
    }

    pub(super) fn release(&self) {
        let _ =
            self.state
                .compare_exchange(CLAIMED, AVAILABLE, Ordering::AcqRel, Ordering::Acquire);
    }
}
