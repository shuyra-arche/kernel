use alloc::sync::Arc;
use alloc::vec::Vec;

use kernel_api::abi::call::pend::PendFail;
use kernel_api::abi::call::share_view::ShareCall;
use kernel_api::abi::call::share_view::ShareFail;
use kernel_api::model::cell::SlotId;
use kernel_api::model::identity::Mark;
use kernel_api::model::identity::PendId;
use kernel_api::model::view::ViewKind;

use super::gate;
use super::outcome::Outcome;
use super::outcome::answer;
use super::outcome::answer_pair;
use crate::basis::nest::Access;
use crate::basis::pend;
use crate::basis::pend::Modifiable;
use crate::basis::pend::Need;
use crate::basis::room::conductor::current;
use crate::basis::unit::task::TaskIdent;
use crate::basis::view;
use crate::basis::view::View;
use crate::machine::switcher::context::TrapContext;

pub(super) fn dispatch(frame: &mut TrapContext, call: ShareCall, ident: Arc<TaskIdent>) -> Outcome {
    match call {
        ShareCall::Create {
            reference,
            modifiable,
        } => {
            let result = current()
                .running_task()
                .ok_or(PendFail::Denied)
                .and_then(|task| create_share(&task, reference, modifiable));
            answer(frame, result.map(|token| token.get()).map_err(fail));
        }
        ShareCall::Delete { token } => {
            answer(
                frame,
                super::seal::seal_resource(token, ViewKind::Share).map_err(fail),
            );
        }
        ShareCall::Open { token } => {
            answer_pair(frame, open_resource(ident, token).map_err(fail));
        }
        ShareCall::Shut { token } => {
            answer(frame, shut_resource(token).map_err(fail));
        }
    }
    Outcome::Resume
}

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
        return pend::accord(
            task,
            parent,
            &Arc::downgrade(task),
            requested,
            Mark::NONE,
            pend::Window::Source(offset, count),
        )
        .map(PendId::mint);
    };
    let _serial = task.pend.lock();
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
        let meta = view::share::ShareView::from_source(source.clone(), size, task.ident.id)?;
        let modifiable = requested;
        if modifiable == Modifiable::Yes && !hold.ceiling().contains(Access::WRITE) {
            return Err(PendFail::Denied);
        }
        let pend =
            pend::PendEntry::try_new(View::Share(meta.clone()), Mark::NONE, modifiable, None)?;
        let token = pend.token();
        let mut inputs = Vec::new();
        for space in authorization.spaces()? {
            let entries = space.mappings.owned_by(&authorization)?;
            inputs
                .try_reserve(entries.len())
                .map_err(|_| PendFail::OoM)?;
            for entry in entries {
                if let Some(mapping) = entry.mapping() {
                    inputs.push((space.clone(), mapping.clone()));
                }
            }
        }
        meta.reserve_records(inputs.len())?;
        task.pend
            .entries
            .lock()
            .try_reserve(1)
            .map_err(|_| PendFail::OoM)?;

        let _commit = crate::basis::commit();
        let _publication = authorization.begin_withdraw()?;
        if !pend::live(task) || !authorization.private_owner(task.ident.id) {
            return Err(PendFail::Denied);
        }
        meta.attach_source(&authorization);
        authorization.adopt(&meta, token, pend.authority().clone())?;
        let transition = (|| {
            for (space, mapping) in inputs {
                meta.adopt_input(token, &space, mapping);
            }
            if modifiable == Modifiable::No {
                meta.close_maps(Some(token))?;
            }
            let mut entries = task.pend.entries.lock();
            assert!(
                entries.len() < entries.capacity(),
                "reserved root PendEntry slot"
            );
            meta.commit_source();
            authorization.consume();
            entries.push(pend);
            pend::changed(task);
            Ok(token)
        })();
        if transition.is_err() {
            authorization.restore_private(source.clone());
        }
        transition
    })();
    if result.is_err() {
        authorization.release_claim();
    }
    result
}

fn open_resource(ident: Arc<TaskIdent>, token: PendId) -> Result<(usize, usize), PendFail> {
    let pend = gate::accede(token, Need::Read)?;
    gate::usable(&pend)?;
    let p = pend.view().share_view().ok_or(PendFail::Denied)?;
    let access = subset_to_access(pend.modifiable())?;
    view::share::open(&p, token, &ident.team.space, access)
}

fn shut_resource(token: PendId) -> Result<usize, PendFail> {
    let task = current().running_task().ok_or(PendFail::Denied)?;
    let pend = pend::locate(&task, token).ok_or(PendFail::Denied)?;
    if !pend.allows(Need::Read) {
        return Err(PendFail::Denied);
    }
    gate::usable(&pend)?;
    let p = pend.view().share_view().ok_or(PendFail::Denied)?;
    view::share::shut(&p, token).map(|()| 0)
}

fn fail(error: PendFail) -> ShareFail {
    match error {
        PendFail::Denied | PendFail::Invalid => ShareFail::Denied,
        PendFail::NotAligned => ShareFail::NotAligned,
        PendFail::Dead => ShareFail::Dead,
        PendFail::OoM => ShareFail::OoM,
        PendFail::HandedOver => ShareFail::HandedOver,
        PendFail::Busy => ShareFail::Busy,
    }
}
