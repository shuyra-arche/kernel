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

impl WaitFail for kernel_api::abi::call::clone_view::CloneFail {
    fn busy() -> Self {
        kernel_api::abi::call::clone_view::CloneFail::Busy
    }
    fn oom() -> Self {
        kernel_api::abi::call::clone_view::CloneFail::OoM
    }
}

impl WaitFail for kernel_api::abi::call::sign::SignFail {
    fn busy() -> Self {
        kernel_api::abi::call::sign::SignFail::Busy
    }
    fn oom() -> Self {
        kernel_api::abi::call::sign::SignFail::OoM
    }
}

impl WaitFail for kernel_api::abi::call::share_view::ShareFail {
    fn busy() -> Self {
        kernel_api::abi::call::share_view::ShareFail::Busy
    }
    fn oom() -> Self {
        kernel_api::abi::call::share_view::ShareFail::OoM
    }
}
