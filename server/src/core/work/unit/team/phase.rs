/// 团队的相位：活着、被收下、注定要死，或已被撤离。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TeamPhase {
    Live,
    Paused,
    Doomed,
    Ousted,
}

impl TeamPhase {
    /// Pure group-control decision. The Team applies scheduling effects only
    /// after publishing the selected phase under the commit boundary.
    pub(crate) fn transition(
        self,
        request: crate::core::work::unit::control::Request,
    ) -> Result<Self, kernel_api::abi::call::unit::UnitFail> {
        use crate::core::work::unit::control::Request;
        match (self, request) {
            (Self::Doomed | Self::Ousted, _) => Err(kernel_api::abi::call::unit::UnitFail::Denied),
            (_, Request::Pause) => Ok(Self::Paused),
            (_, Request::Resume) => Ok(Self::Live),
        }
    }

    /// 注定要死或已被撤离：这两种相位都不再接新的事。
    pub(crate) fn terminal(&self) -> bool {
        matches!(self, TeamPhase::Doomed | TeamPhase::Ousted)
    }

    /// 已被撤离：名字与家长位都不再指向它。
    pub(crate) fn ousted(&self) -> bool {
        matches!(self, TeamPhase::Ousted)
    }
}
