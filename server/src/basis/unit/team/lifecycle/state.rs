use kernel_api::abi::call::unit::UnitFail;

use crate::basis::unit::control::Request;

/// 团队的相位：活着、被收下、注定要死，或已被撤离。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TeamPhase {
    Live,
    Paused,
    Doomed,
    Ousted,
}

impl TeamPhase {
    /// Pure group-control decision. The Team applies scheduling effects only
    /// after publishing the selected phase under the commit boundary.
    fn transition(self, request: Request) -> Result<Self, UnitFail> {
        match (self, request) {
            (Self::Doomed | Self::Ousted, _) => Err(UnitFail::Denied),
            (_, Request::Pause) => Ok(Self::Paused),
            (_, Request::Resume) => Ok(Self::Live),
        }
    }

    /// 注定要死或已被撤离：这两种相位都不再接新的事。
    fn terminal(self) -> bool {
        matches!(self, Self::Doomed | Self::Ousted)
    }
}

/// 团队的阶段：还在构造（没有默认入口）或已就绪。
pub(crate) enum TeamStage {
    Constructing,
    Ready { default_entry: usize },
}

// Membership admission is independent of whether existing executions may run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Admission {
    Open,
    Closed,
}

/// 团队状态机：阶段（`TeamStage`）、相位（`TeamPhase`）与成员准入。
pub(crate) struct TeamState {
    stage: TeamStage,
    admission: Admission,
    phase: TeamPhase,
}

unsafe impl Send for TeamState {}

impl TeamState {
    pub(crate) fn new(stage: TeamStage) -> TeamState {
        TeamState {
            stage,
            admission: Admission::Open,
            phase: TeamPhase::Live,
        }
    }

    pub(crate) fn terminal(&self) -> bool {
        self.phase.terminal()
    }

    pub(crate) fn admitting(&self) -> bool {
        self.admission == Admission::Open
    }

    pub(crate) fn paused(&self) -> bool {
        self.phase == TeamPhase::Paused
    }

    pub(crate) fn ousted(&self) -> bool {
        self.phase == TeamPhase::Ousted
    }

    pub(crate) fn finish(&mut self) {
        self.admission = Admission::Closed;
    }

    pub(crate) fn doom(&mut self) {
        if !self.terminal() {
            self.finish();
            self.phase = TeamPhase::Doomed;
        }
    }

    /// The coordinator has retired the Nest and checked the completion cohort.
    pub(crate) fn oust(&mut self) -> Result<(), UnitFail> {
        if !self.terminal() {
            return Err(UnitFail::Busy);
        }
        self.discard();
        Ok(())
    }

    /// Final Team drop can also discard an unpublished or empty live Team.
    pub(crate) fn discard(&mut self) {
        self.finish();
        self.phase = TeamPhase::Ousted;
    }

    pub(crate) fn ready(&self) -> bool {
        matches!(self.stage, TeamStage::Ready { .. }) && !self.terminal()
    }

    pub(crate) fn constructing(&self) -> bool {
        matches!(self.stage, TeamStage::Constructing) && !self.terminal()
    }

    pub(crate) fn default_entry(&self) -> usize {
        match (&self.stage, self.terminal()) {
            (TeamStage::Ready { default_entry }, false) => *default_entry,
            _ => 0,
        }
    }

    pub(crate) fn set_default_entry(&mut self, va: usize) {
        assert!(
            !self.terminal()
                && matches!(
                    self.stage,
                    TeamStage::Constructing | TeamStage::Ready { default_entry: 0 }
                ),
            "default entry can only be installed on a live Team with no entry"
        );
        self.stage = TeamStage::Ready { default_entry: va };
    }

    /// 收下：活着就转暂停，已经暂停则无操作；注定要死或已撤离不允许。
    pub(crate) fn debark(&mut self) -> Result<(), UnitFail> {
        self.phase = self.phase.transition(Request::Pause)?;
        Ok(())
    }

    /// 换回：暂停就恢复活着，活着则无操作；注定要死或已撤离不允许。
    pub(crate) fn embark(&mut self) -> Result<(), UnitFail> {
        self.phase = self.phase.transition(Request::Resume)?;
        Ok(())
    }
}
