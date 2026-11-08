use alloc::sync::Arc;
use kernel_api::abi::call::unit::UnitFail;
mod target;
pub(crate) use target::JoinTarget;
pub(crate) fn observe(
    me: &Arc<crate::basis::unit::task::Task>,
    target: kernel_api::model::unit::UnitTarget,
) -> Result<JoinTarget, UnitFail> {
    let _commit = crate::basis::commit();
    match target {
        kernel_api::model::unit::UnitTarget::Team(id) => {
            if id == me.ident.team.id {
                return Err(UnitFail::Denied);
            }
            me.clan(id)
                .map(|team| JoinTarget::Team(team.life.clone()))
                .ok_or(UnitFail::Denied)
        }
        kernel_api::model::unit::UnitTarget::Task(id) => {
            let member = me
                .ident
                .team
                .life
                .member(id)
                .or_else(|| me.ident.team.life.find(id));
            member
                .filter(|member| member.pending(me.ident.id))
                .map(JoinTarget::Task)
                .ok_or(UnitFail::Denied)
        }
    }
}
