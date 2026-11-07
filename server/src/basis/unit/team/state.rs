use kernel_api::abi::call::unit::UnitFail;

use crate::basis::unit::control::Request;

use super::TeamPhase;
use super::TeamStage;

/// 团队状态机：阶段（`TeamStage`）、相位（`TeamPhase`）与成员准入。
pub(crate) struct TeamState {
    pub(crate) stage: TeamStage,
    pub(crate) admission: super::admission::Admission,
    pub(crate) phase: TeamPhase,
}

unsafe impl Send for TeamState {}

impl TeamState {
    pub(crate) fn new(stage: TeamStage) -> TeamState {
        TeamState {
            stage,
            admission: super::admission::Admission::Open,
            phase: TeamPhase::Live,
        }
    }

    pub(crate) fn terminal(&self) -> bool {
        self.phase.terminal()
    }

    pub(crate) fn debarked(&self) -> bool {
        matches!(self.phase, TeamPhase::Paused)
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
