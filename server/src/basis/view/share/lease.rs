//! Authorized source adoption and CPU borrowing transactions.
use alloc::sync::Arc;

use kernel_api::abi::call::pend::PendFail;
use kernel_api::model::cell::SlotId;
use kernel_api::model::identity::Mark;
use kernel_api::model::identity::PendId;
use kernel_api::model::pend::{Operations, PendRange, PendScope, ShareRights};

use crate::basis::nest::Access;
use crate::basis::pend;
use crate::basis::pend::Modifiable;
use crate::basis::pend::Need;
use crate::basis::pend::access as authority;
use crate::basis::room::scheduler::slot::current;
use crate::basis::unit::task::TaskIdent;
use crate::basis::view::View;

fn subset_to_access(subset: Modifiable) -> Result<Access, PendFail> {
    if !subset.allows(Modifiable::No) {
        return Err(PendFail::Denied);
    }
    let mut access = Access::READ;
    if subset == Modifiable::Yes {
        access |= Access::WRITE;
    }
    Ok(access)
}

pub(crate) fn create_share(
    task: &Arc<crate::basis::unit::task::Task>,
    license: PendId,
    reference: SlotId,
    requested: Modifiable,
) -> Result<PendId, PendFail> {
    if !pend::live(task) {
        return Err(PendFail::Dead);
    }
    let hold = task
        .pend
        .references
        .get(reference.get())
        .ok_or(PendFail::Denied)?;
    let authorization = hold.source().clone();
    // 已交给根 Pend 的来源没有私有阶段：只能按**子段引用**派生一枚独立子资源。
    let Some(source) = authorization.own_range() else {
        let (parent, _) = hold.branch().ok_or(PendFail::Denied)?;
        let (offset, count) = hold.window();
        let entry = pend::locate(task, parent).ok_or(PendFail::Denied)?;
        let mut limits = entry.authority().limits();
        if let Operations::Flat(rights) = limits.operations {
            use kernel_api::model::pend::FlatRights;
            if !rights.contains(FlatRights::SHARE) {
                return Err(PendFail::Denied);
            }
            let mut rights_out = ShareRights::empty();
            if rights.contains(FlatRights::ASSOCIATE) {
                rights_out |= ShareRights::association();
            }
            if rights.contains(FlatRights::DELETE) {
                rights_out |= ShareRights::DELETE;
            }
            limits.operations = Operations::Share(rights_out);
        }
        return pend::accord(
            task,
            parent,
            &Arc::downgrade(task),
            kernel_api::model::pend::Limits {
                modifiable: requested,
                scope: PendScope::Pages(PendRange::Inherit),
                ..limits
            },
            Mark::NONE,
            pend::Window::Source(offset, count),
            Some((
                license,
                Operations::None(kernel_api::model::pend::CreationRights::SHARE),
            )),
        )
        .map(PendId::mint);
    };
    let _serial = task.pend.lock();
    authority::license(
        task,
        license,
        kernel_api::model::pend::CreationRights::SHARE,
    )?;
    // 只能从**私有根**铸新根：子 PendEntry / Share 派生 / 借用都不产生新根资格。
    let (offset, count) = hold.window();
    if !authorization.private_owner(task.ident.id)
        || hold.branch().is_some()
        || offset.get() != 0
        || count != source.page_count()
    {
        return Err(PendFail::Denied);
    }
    // 一次性创建资格：单次 claim，竞争方 Busy、已消费 Denied。
    authorization.claim()?;
    let result = (|| {
        let size = source
            .page_count()
            .checked_bytes()
            .ok_or(PendFail::NotAligned)?;
        let meta =
            crate::basis::flat::sharing::Sharing::from_source(source.clone(), size, task.ident.id)?;
        let modifiable = requested;
        if modifiable == Modifiable::Yes && !hold.ceiling().contains(Access::WRITE) {
            return Err(PendFail::Denied);
        }
        let pend =
            pend::PendEntry::try_new(View::Share(meta.clone()), Mark::NONE, modifiable, None)?;
        let pend = Arc::try_new(pend).map_err(|_| PendFail::OutOfMemory)?;
        let token = pend.token();
        let registration = task.pend.prepare(pend.clone())?;
        // The creation claim closes private-source admission until adoption.
        crate::basis::nest::authority::withdraw_source(&authorization, false)?;
        let _commit = crate::basis::commit();
        let withdrawal = authorization.begin_withdraw()?;
        if !pend::live(task) || !authorization.private_owner(task.ident.id) {
            return Err(PendFail::Denied);
        }
        meta.attach_source(&authorization);
        withdrawal.adopt(&meta, token, pend.authority().clone())?;
        authorization.consume();
        registration.publish();
        Ok(token)
    })();
    if result.is_err() {
        authorization.release_claim();
    }
    result
}

pub(crate) fn open_resource(
    ident: Arc<TaskIdent>,
    token: PendId,
) -> Result<(usize, usize), PendFail> {
    let pend = authority::accede(token, Need::Read)?;
    authority::usable(&pend)?;
    if !pend.permits(Operations::Share(ShareRights::OPEN)) {
        return Err(PendFail::Denied);
    }
    let p = pend.target().share_view().ok_or(PendFail::Denied)?;
    let access = subset_to_access(pend.modifiable())?;
    crate::basis::flat::sharing::open(&p, token, &ident.team.nest, access)
}

pub(crate) fn shut_resource(token: PendId) -> Result<usize, PendFail> {
    let task = current().running_task().ok_or(PendFail::Denied)?;
    let pend = pend::locate(&task, token).ok_or(PendFail::Denied)?;
    if !pend.allows(Need::Read) || !pend.permits(Operations::Share(ShareRights::SHUT)) {
        return Err(PendFail::Denied);
    }
    authority::usable(&pend)?;
    let p = pend.target().share_view().ok_or(PendFail::Denied)?;
    crate::basis::flat::sharing::shut(&p, token).map(|()| 0)
}
