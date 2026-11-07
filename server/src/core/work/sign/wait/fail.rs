pub(crate) trait WaitFail: kernel_api::abi::error::FailCode {
    fn busy() -> Self;
    fn oom() -> Self;
}

impl WaitFail for kernel_api::abi::call::room::RoomFail {
    fn busy() -> Self {
        kernel_api::abi::call::room::RoomFail::Busy
    }
    fn oom() -> Self {
        kernel_api::abi::call::room::RoomFail::OoM
    }
}

impl WaitFail for kernel_api::abi::call::unit::UnitFail {
    fn busy() -> Self {
        kernel_api::abi::call::unit::UnitFail::Busy
    }
    fn oom() -> Self {
        kernel_api::abi::call::unit::UnitFail::OoM
    }
}

impl WaitFail for kernel_api::abi::call::hole::HoleFail {
    fn busy() -> Self {
        kernel_api::abi::call::hole::HoleFail::Busy
    }
    fn oom() -> Self {
        kernel_api::abi::call::hole::HoleFail::OoM
    }
}

impl WaitFail for kernel_api::abi::call::nole::NoleFail {
    fn busy() -> Self {
        kernel_api::abi::call::nole::NoleFail::Busy
    }
    fn oom() -> Self {
        kernel_api::abi::call::nole::NoleFail::OoM
    }
}

impl WaitFail for kernel_api::abi::call::pole::PoleFail {
    fn busy() -> Self {
        kernel_api::abi::call::pole::PoleFail::Busy
    }
    fn oom() -> Self {
        kernel_api::abi::call::pole::PoleFail::OoM
    }
}

impl WaitFail for kernel_api::abi::call::tole::ToleFail {
    fn busy() -> Self {
        kernel_api::abi::call::tole::ToleFail::Busy
    }
    fn oom() -> Self {
        kernel_api::abi::call::tole::ToleFail::OoM
    }
}
