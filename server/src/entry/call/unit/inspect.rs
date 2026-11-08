use super::*;
use crate::basis::unit::team::{Member, Team};
use kernel_api::model::identity::{TaskId, TeamId};
pub(super) fn query(me: &Arc<Task>, query: UnitQuery) -> Result<Option<UnitInfo>, UnitFail> {
    let _commit = crate::basis::commit();
    let target = match query {
        UnitQuery::Caller => UnitTarget::Task(me.ident.id),
        UnitQuery::Target(target) => target,
        UnitQuery::Children { after } => {
            let next = me
                .heir
                .lock()
                .iter()
                .filter(|team| team.id.get() > after.get())
                .min_by_key(|team| team.id.get())
                .cloned();
            return Ok(next.map(|team| team_info(&team)));
        }
        UnitQuery::Members { team, after } => {
            let team = team_for(me, team)?;
            let next = team
                .life
                .tasks
                .lock()
                .iter()
                .filter(|m| {
                    m.id.get() > after.get() && (m.pending(me.ident.id) || m.id == me.ident.id)
                })
                .min_by_key(|m| m.id.get())
                .cloned();
            return Ok(next.map(|member| task_info(&member)));
        }
    };
    match target {
        UnitTarget::Team(id) => Ok(Some(team_info(&team_for(me, id)?))),
        UnitTarget::Task(id) => {
            let member = if id == me.ident.id {
                me.ident.team.life.member(id).ok_or(UnitFail::Denied)?
            } else {
                let crate::basis::unit::join::JoinTarget::Task(member) =
                    crate::basis::unit::join::observe(me, target)?
                else {
                    unreachable!()
                };
                member
            };
            Ok(Some(task_info(&member)))
        }
    }
}
fn team_for(me: &Arc<Task>, id: TeamId) -> Result<Arc<Team>, UnitFail> {
    if id == me.ident.team.id {
        Ok(me.ident.team.clone())
    } else {
        me.clan(id).ok_or(UnitFail::Denied)
    }
}
fn team_info(team: &Arc<Team>) -> UnitInfo {
    let admitting = team.life.admitting();
    let state = if !admitting && team.all_reaped() {
        UnitState::Exited
    } else if team.life.closed() {
        UnitState::Stopping
    } else if team.paused() {
        UnitState::Frozen
    } else {
        UnitState::Ready
    };
    UnitInfo {
        target: UnitTarget::Team(team.id),
        team: team.id,
        controller: team.sire().unwrap_or(TaskId::new(0)),
        creator: team.creator(),
        sire: team.sire().unwrap_or(TaskId::new(0)),
        state,
        reclaimed: false,
        admitting,
        exit: None,
        result: None,
    }
}
fn task_info(member: &Member) -> UnitInfo {
    let state = member.state.lock();
    let phase = if state.is_reaped() {
        UnitState::Exited
    } else if state.is_doomed() {
        UnitState::Stopping
    } else if state.paused() || state.is_parked() {
        UnitState::Frozen
    } else if state.is_held() {
        UnitState::Dormant
    } else if state.is_waiting() {
        UnitState::Waiting
    } else if state.on_hart().is_some() {
        UnitState::Running
    } else {
        UnitState::Ready
    };
    let exit = state.exit().map(|(cause, reason)| (cause, reason as u64));
    drop(state);
    UnitInfo {
        target: UnitTarget::Task(member.id),
        team: member.team,
        controller: member.controller,
        creator: member.creator,
        sire: member.sire,
        state: phase,
        reclaimed: member.reclaimed.load(Ordering::Acquire),
        admitting: false,
        exit,
        result: if exit.is_some() {
            member.result.lock().map(|value| value as u64)
        } else {
            None
        },
    }
}
