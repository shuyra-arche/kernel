use alloc::sync::Arc;
use alloc::sync::Weak;
use alloc::vec::Vec;

use kernel_api::abi::call::cell::CellCall;
use kernel_api::abi::call::cell::CellFail;
use kernel_api::model::cell::AcquireSource;
use kernel_api::model::modifiable::Modifiable;

use crate::basis::cell::operation::Transfer;
use crate::basis::cell::operation::TransferError;
use crate::basis::flat::Allocation;
use crate::basis::flat::Flat;
use crate::basis::flat::FlatError;
use crate::basis::flat::FlatRange;
use crate::basis::nest::Access;
use crate::basis::nest::Mapping;
use crate::basis::nest::MappingRange;
use crate::basis::nest::Nest;
use crate::basis::nest::NestError;
use crate::basis::nest::Placement;
use crate::basis::nest::PlacementAt;
use crate::basis::nest::RegionId;
use crate::basis::nest::Reservation;
use crate::basis::pend;
use crate::basis::pend::Geometry;
use crate::basis::pend::Hold;
use crate::basis::pend::IndependentHold;
use crate::basis::pend::SourceAuthorization;
use crate::basis::sign;
use crate::basis::sign::WakeKey;
use crate::basis::unit::space::MappingEntry;
use crate::basis::unit::space::Space;
use crate::basis::unit::task::Task;
use crate::basis::unit::task::TaskIdent;
use crate::basis::unit::team::Team;
use crate::basis::view::share_view::ShareOperation;
use crate::machine::mmu::riscv::identity;
use crate::machine::switcher::context::TrapContext;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PageCount;
use kernel_api::address::PageOffset;
use kernel_api::address::VirtAddr as KVirt;

use super::outcome::answer;
use super::outcome::answer_pair;

pub(super) fn dispatch(frame: &mut TrapContext, call: CellCall, ident: &Arc<TaskIdent>) {
    match call {
        CellCall::Attach { target, request } => {
            answer_pair(frame, attach_target(target, request, ident));
        }
        CellCall::Inspect { target, mapping } => {
            let result = (|| {
                let actor = caller(ident).ok_or(CellFail::Denied)?;
                let _serial = actor.pend.lock();
                let _commit = crate::basis::commit();
                let kernel_api::model::cell::MappingTarget::Io(token) = target else {
                    return Err(CellFail::Denied);
                };
                crate::basis::unit::space::external::inspect(&actor, token, mapping)
            })();
            match result {
                Ok(info) => {
                    use crate::machine::switcher::context::Gprs;
                    frame.gpr.set_x(Gprs::A0, info.state as usize);
                    frame.gpr.set_x(Gprs::A1, info.fault_cause);
                    frame.gpr.set_x(Gprs::A2, info.fault_address);
                }
                Err(e) => super::outcome::ret_err(frame, e),
            }
        }
        _ => answer(frame, execute(call, ident)),
    }
}

// MappingId locates an address projection; access and control are checked separately.
fn execute(call: CellCall, ident: &Arc<TaskIdent>) -> Result<usize, CellFail> {
    match call {
        CellCall::Acquire { source, retention } => acquire(source, retention, ident),
        CellCall::Release { reference } => release(reference, ident),
        CellCall::Detach {
            target,
            mapping,
            selection,
        } => match target {
            kernel_api::model::cell::MappingTarget::Cpu(team) => {
                detach(team, mapping, selection, ident)
            }
            kernel_api::model::cell::MappingTarget::Io(token) => {
                if selection != kernel_api::model::cell::MappingSelection::Whole {
                    return Err(CellFail::Denied);
                }
                let actor = caller(ident).ok_or(CellFail::Denied)?;
                let _serial = actor.pend.lock();
                let _commit = crate::basis::commit();
                crate::basis::unit::space::external::detach(&actor, token, mapping)?;
                Ok(0)
            }
        },
        CellCall::SetAccess {
            team,
            mapping,
            selection,
            access,
        } => set_access(team, mapping, selection, access, ident),
        CellCall::Traverse {
            retention,
            range,
            out,
        } => traverse(retention, range, out, ident),
        CellCall::Push {
            source,
            page,
            target,
            change,
        } => push(source, page, target, change, ident),
        CellCall::Pull {
            source,
            destination,
            page,
            change,
        } => pull(source, destination, page, change, ident),
        CellCall::Begin { source, page } => begin(source, page, ident),
        CellCall::End { change } => end(change, ident),
        CellCall::Attach { .. } | CellCall::Inspect { .. } => {
            unreachable!("Attach answers a pair")
        }
    }
}

/// 目标域：当前域，或调用者的 Constructing 子域（非零 Team 还要求仍在构造中）。
fn resolve_team(
    team: kernel_api::model::identity::TeamId,
    ident: &Arc<TaskIdent>,
    caller: &Arc<Task>,
) -> Result<Arc<Team>, CellFail> {
    if team.get() == 0 {
        return Ok(ident.team.clone());
    }
    let target = caller.clan(team).ok_or(CellFail::Denied)?;
    if !target.constructing() {
        return Err(CellFail::Denied);
    }
    Ok(target)
}

/// 从**调用者**地址空间一次快照一段定长只读描述。
fn read_snapshot<const N: usize>(space: &Space, at: KVirt) -> Result<[u8; N], CellFail> {
    let mut bytes = [0; N];
    if !space.copy_in(&mut bytes, at) {
        return Err(CellFail::Denied);
    }
    Ok(bytes)
}

/// 把线外的选址描述换成 Nest 的选址；长度不在这里——由来源范围定。
fn nest_placement(
    nest: &Nest,
    placement: kernel_api::model::cell::Placement,
    size: usize,
) -> Result<Placement, CellFail> {
    let at = match placement.at() {
        kernel_api::model::cell::PlacementAt::Lowest => PlacementAt::Lowest,
        kernel_api::model::cell::PlacementAt::Highest => PlacementAt::Highest,
        kernel_api::model::cell::PlacementAt::Exact(address) => {
            let address = KVirt::wrap(address.get());
            if !Space::user_range(address, size) {
                return Err(CellFail::NotAligned);
            }
            if !nest.window_contains(RegionId::Normal, address, size) {
                return Err(CellFail::NoRegion);
            }
            PlacementAt::Exact(address)
        }
    };
    Placement::new(RegionId::Normal, size, placement.align(), at)
        .map_err(|e| CellFail::from(NestError::from(e)))
}

/// 回滚 `Attach`：CPU 关系结清后才撤登记与控制。
///
/// `reservation` 是未发布的预留；否则 `mapping` 交给 Nest 撤图，
/// 无 `mapping` 表示发布失败的责任已留在 Nest 清理链。两种凭证不能同时给出。
/// 不主动重试或覆盖原始失败；未结责任由后续入口推进。
/// 结清那一步走 `retire_mapping` 同一条规则：登记、源侧成员与 controller 一起撤。
pub(in crate::entry) fn rollback(
    space: &Space,
    id: usize,
    reservation: Option<Reservation>,
    mapping: Option<Arc<Mapping>>,
) {
    if let Some(reservation) = reservation {
        debug_assert!(mapping.is_none(), "one attempt has one side to unwind");
        let _ = space.nest.release_reservation(reservation);
    } else if let Some(mapping) = mapping {
        let _ = space.dispose(mapping);
    }
    if !space.nest.settled(id) {
        return;
    }
    space.retire_mapping(id);
}

/// `Attach`：预留地址并建立翻译关系，返回 `(关系句柄, 视图起点)`。
///
/// 发布与撤销共取来源授权上那一把闸门：发布前确认没被撤销，发布后再复查一次；
/// 撤销方在同一把门下置位。于是撤销要么看见这次发布、要么发布自己发现已被撤销。
fn attach(
    range: kernel_api::model::cell::ReferenceRange,
    team: kernel_api::model::identity::TeamId,
    placement: kernel_api::model::cell::Placement,
    access: u64,
    control: kernel_api::model::cell::MappingControl,
    ident: &Arc<TaskIdent>,
) -> Result<(usize, usize), CellFail> {
    let caller_task = caller(ident).ok_or(CellFail::Denied)?;
    let access = Access::from_wire(access).ok_or(CellFail::Denied)?;
    // 空权限拒。执行权不由 Pend 的 Modifiable表达：它只随**目标域授权**
    // 与来源上限走，所以放到 `resolve_team` 之后判。
    if access.is_empty() {
        return Err(CellFail::Denied);
    }
    let target = resolve_team(team, ident, &caller_task)?;
    let _construction = if team.get() != 0 {
        Some(target.operation().ok_or(CellFail::Busy)?)
    } else {
        None
    };
    if team.get() != 0 && !target.constructing() {
        return Err(CellFail::Denied);
    }
    // 当前域不开执行权（旧 `Allocate` 给的也是 RW）；调用者的 Constructing
    // 子域按上限开——装载器就是走这条路装代码段。
    if access.contains(Access::EXECUTE) && team.get() == 0 {
        return Err(CellFail::Denied);
    }
    let space = target.space.clone();
    // **本域**还是**交付到别的域**：交付出去的关系不算调用者的映射，不得挂在调用者
    // 那条可撤链上（否则来源回收 / 撤销会把别人正在跑的代码拆掉）。
    let own_space = team.get() == 0;
    // 引用表下只取一份合法持有快照，随后立刻放锁：核心工作（Nest 同步 / 挂载）
    // 不得带着资源表锁等待；并发的 Release 只会把它从表里摘走，这份快照仍保有来源。
    let hold = {
        let _serial = caller_task.pend.lock();
        caller_task
            .pend
            .references
            .get(range.reference.get())
            .ok_or(CellFail::Denied)?
    };
    let count = PageCount::new(range.count).ok_or(CellFail::NotAligned)?;
    // 这次要挂的是引用窗口里的一段：相对来源起点的页窗口，只加这一层引用偏移。
    let used = PageOffset::new(
        hold.window()
            .0
            .get()
            .checked_add(range.offset)
            .ok_or(CellFail::NotAligned)?,
    );
    // 持有人自己那条独立持有上的映射不算对父段的新使用（它就靠这条引用拿帧）；
    // 豁免按**这一份登记的身份**认，不按任务。
    let own = hold.retention();
    let source = hold
        .resolve()
        .ok_or(CellFail::Denied)?
        .range(PageOffset::new(range.offset), count)
        .map_err(|_| CellFail::NoRegion)?;
    let size = count.checked_bytes().ok_or(CellFail::NotAligned)?;
    let authorization = hold.source().clone();
    let root = authorization.root();
    let branch = hold
        .branch()
        .map(|(token, _)| token)
        .or_else(|| root.as_ref().map(|(_, token, _)| *token));
    let share_view = root.map(|(meta, _, _)| meta);
    let authority = hold.authority().cloned().or_else(|| {
        share_view
            .as_ref()
            .zip(branch)
            .and_then(|(meta, token)| meta.authority(token))
    });
    let _operation = share_view
        .as_ref()
        .map(|meta| ShareOperation::acquire(meta).ok_or(CellFail::Busy))
        .transpose()?;
    // Revalidate after acquiring ShareOperation: Narrow may have run after the initial resolution.
    hold.resolve().ok_or(CellFail::Denied)?;
    let modifiable = match &authority {
        Some(authority) => authority.modifiable(),
        None => authorization.modifiable(),
    }
    .ok_or(CellFail::Denied)?;
    if !modifiable.allows(Modifiable::No)
        || (access.contains(Access::WRITE) && modifiable != Modifiable::Yes)
        || (!own_space && !modifiable.allows(Modifiable::No))
    {
        return Err(CellFail::Denied);
    }
    if !hold.ceiling().contains(access) {
        return Err(CellFail::WidenDenied);
    }
    let mut ceiling = hold.ceiling();
    if modifiable != Modifiable::Yes {
        ceiling.remove(Access::WRITE);
    }
    if !own_space {
        ceiling &= access;
    } else if share_view.is_some() {
        ceiling &= Access::READ | Access::WRITE;
    }
    if let Some(meta) = &share_view {
        if !meta.alive() {
            return Err(CellFail::Denied);
        }
        // Bootstrap executable/read-only mappings remain attached to their Pend.
        // Writable cross-Unit data requires a Pend delegated to the receiving Task.
        if !own_space
            && (modifiable != Modifiable::No || !meta.readonly() || access.contains(Access::WRITE))
        {
            return Err(CellFail::Denied);
        }
    } else if !authorization.private_owner(ident.id) || !own_space {
        return Err(CellFail::Denied);
    }
    let nest = &space.nest;
    nest.retry_invalidation()?;
    space.reap_mappings();
    // 分支下的登记位先备好（只在本域安装时才需要）：提交时只替换，不再可能失败。
    if let Some(meta) = &share_view {
        meta.reserve_records(1).map_err(source_fail)?;
    }
    let reservation = nest.reserve(nest_placement(nest, placement, size)?)?;
    let id = reservation.id();
    // 号必须在符号上界内：过界就当场交还这笔占用，不拿它去写 PTE。
    if id > isize::MAX as usize {
        rollback(&space, id, Some(reservation), None);
        return Err(CellFail::OoM);
    }
    // 发布前先占住登记槽、来源成员位与在途号：容量都在这一处备齐，提交只填已备位置。
    if let Err(error) =
        space
            .mappings
            .claim(id, &space, ident.id, control, authorization.clone(), branch)
    {
        rollback(&space, id, Some(reservation), None);
        return Err(source_fail(error));
    }
    if control == kernel_api::model::cell::MappingControl::Caller
        && let Err(error) = caller_task.track_control(&space, id)
    {
        rollback(&space, id, Some(reservation), None);
        return Err(error.into());
    }
    if let Err(error) = authorization.enter_publish(id, authority.as_deref(), access) {
        rollback(&space, id, Some(reservation), None);
        return Err(source_fail(error));
    }
    let mapping = match nest.attach(reservation, source, ceiling, access) {
        Ok(mapping) => mapping,
        Err(error) => {
            authorization.leave_publish(id);
            rollback(&space, id, None, None);
            return Err(error.into());
        }
    };
    let address = mapping.initial_start().as_usize();
    // 这段关系带执行权：取完指令缓存才算可用。
    if access.contains(Access::EXECUTE)
        && let Err(error) = identity::sync_instructions()
    {
        rollback(&space, id, None, Some(mapping));
        authorization.leave_publish(id);
        return Err(error.into());
    }
    let committed = (|| {
        let _serial = caller_task.pend.lock();
        let _commit = crate::basis::commit();
        if !crate::basis::pend::live(&caller_task)
            || !authorization.branch_check(authority.as_deref(), access)
        {
            return Err(CellFail::Denied);
        }
        // 旧引用照旧得过高一道发表准入：已分出的子段不被隐式重映射。
        authorization
            .check_publish(used, count, own)
            .map_err(source_fail)?;
        if let (Some(meta), Some(token)) = (&share_view, branch)
            && meta.record(token, &space, mapping.clone(), false).is_err()
        {
            return Err(CellFail::Denied);
        }
        space.mappings.commit(id, mapping.clone());
        Ok(())
    })();
    if let Err(error) = committed {
        rollback(&space, id, None, Some(mapping));
        authorization.leave_publish(id);
        return Err(error);
    }
    authorization.leave_publish(id);
    Ok((id, address))
}

/// Traverse requires both current authority and a live residency.
///
/// 凭据是**当场拿得出的那枚真保留授权**：只看这份引用“是怎么来的”不够——
/// provenance 只说明存活，不自动赋物理坐标。两层都要过：
/// 活保留 cap（`require_retention`）＋ 这份引用本身就是 cap 登记出来的 retained 引用。
/// 整张表先物化齐了才写出去——失败不在调用者缓冲里留半张。
fn traverse(
    retention: kernel_api::model::unit::ControlId,
    range_at: kernel_api::address::VirtAddr,
    out: kernel_api::address::VirtAddr,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    let task = caller(ident).ok_or(CellFail::Denied)?;
    require_retention(&task, retention)?;
    let space = ident.team.space.clone();
    let bytes = read_snapshot::<{ kernel_api::model::cell::ReferenceRange::WIDTH }>(
        &space,
        KVirt::wrap(range_at.get()),
    )?;
    let range =
        kernel_api::model::cell::ReferenceRange::fetch(&bytes).ok_or(CellFail::NotAligned)?;
    // 引用表下只取一份合法持有快照，随后立刻放锁：物化与写缓冲都不带着资源表锁做。
    let hold = {
        let _serial = task.pend.lock();
        task.pend
            .references
            .get(range.reference.get())
            .ok_or(CellFail::Denied)?
    };
    if !hold.retains() {
        return Err(CellFail::Denied);
    }
    let count = PageCount::new(range.count).ok_or(CellFail::NotAligned)?;
    let source = hold
        .resolve()
        .ok_or(CellFail::Denied)?
        .range(PageOffset::new(range.offset), count)
        .map_err(|_| CellFail::NoRegion)?;
    let pages = source.materialize().map_err(CellFail::from)?;
    let mut list = alloc::vec::Vec::new();
    list.try_reserve_exact(count.get())
        .map_err(|_| CellFail::OoM)?;
    for index in 0..count.get() {
        let address = pages
            .physical(PageOffset::new(index))
            .map_err(CellFail::from)?;
        list.push(address.as_usize() as u64);
    }
    let mut bytes = alloc::vec::Vec::new();
    bytes
        .try_reserve_exact(count.get() * 8)
        .map_err(|_| CellFail::OoM)?;
    for address in list {
        bytes.extend_from_slice(&address.to_le_bytes());
    }
    // 整张表都备好了才提出口：此处失败不碰调用者缓冲，保留仍在。
    if !space.copy_out(&bytes, KVirt::wrap(out.get())) {
        return Err(CellFail::Denied);
    }
    Ok(0)
}

/// 按目标域与号取回控制登记；号本身不授予任何东西。
fn with_mapping<T>(
    team: kernel_api::model::identity::TeamId,
    mapping: kernel_api::model::cell::MappingId,
    ident: &Arc<TaskIdent>,
    action: impl FnOnce(&Arc<Team>, &MappingEntry) -> Result<T, CellFail>,
) -> Result<T, CellFail> {
    let caller_task = caller(ident).ok_or(CellFail::Denied)?;
    let target = resolve_team(team, ident, &caller_task)?;
    let _construction = if team.get() != 0 {
        Some(target.operation().ok_or(CellFail::Busy)?)
    } else {
        None
    };
    if team.get() != 0 && !target.constructing() {
        return Err(CellFail::Denied);
    }
    let space = target.space.clone();
    let entry = space
        .mappings
        .locate(mapping.get())
        .ok_or(CellFail::Denied)?;
    let owned = entry
        .binding
        .space()
        .upgrade()
        .is_some_and(|space| Arc::ptr_eq(&space, &target.space));
    if !owned {
        return Err(CellFail::Denied);
    }
    // 控制者与分支读同一份快照：两段不会来自两次不同的读。
    let control = entry.control.as_ref().ok_or(CellFail::Denied)?;
    let (controller, _) = control.control();
    let allowed = match control.scope() {
        kernel_api::model::cell::MappingControl::Caller => controller == ident.id,
        // 显式把这一段交给本 Space 成员；不是“同 Space 自动全控”。
        kernel_api::model::cell::MappingControl::SpaceMembers => {
            Arc::ptr_eq(&ident.team.space, &space)
        }
    };
    if !allowed {
        return Err(CellFail::Denied);
    }
    action(&target, &entry)
}

/// 把线外的选择换成这一段关系上的身份选择。
fn selection(
    entry: &MappingEntry,
    selection: kernel_api::model::cell::MappingSelection,
) -> Result<MappingRange, CellFail> {
    let mapping = entry.mapping().ok_or(CellFail::Denied)?;
    match selection {
        kernel_api::model::cell::MappingSelection::Whole => Ok(mapping.whole()),
        kernel_api::model::cell::MappingSelection::Pages { offset, count } => {
            let count = PageCount::new(count).ok_or(CellFail::NotAligned)?;
            mapping
                .range(PageOffset::new(offset), count)
                .map_err(|_| CellFail::NoRegion)
        }
    }
}

fn detach(
    team: kernel_api::model::identity::TeamId,
    mapping: kernel_api::model::cell::MappingId,
    choice: kernel_api::model::cell::MappingSelection,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    with_mapping(team, mapping, ident, |target, entry| {
        let space = &target.space;
        space.nest.retry_invalidation()?;
        if !space.nest.settled(mapping.get()) {
            space.withdraw(selection(entry, choice)?)?;
        }
        if space.nest.settled(mapping.get()) {
            space.retire_mapping(mapping.get());
        }
        Ok(0)
    })
}

fn set_access(
    team: kernel_api::model::identity::TeamId,
    mapping: kernel_api::model::cell::MappingId,
    choice: kernel_api::model::cell::MappingSelection,
    access: u64,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    let access = Access::from_wire(access).ok_or(CellFail::Denied)?;
    if !access.is_leaf_set() {
        return Err(CellFail::Denied);
    }
    with_mapping(team, mapping, ident, |target, entry| {
        target.space.nest.retry_invalidation()?;
        target.space.access(selection(entry, choice)?, access)?;
        if access.contains(Access::EXECUTE) {
            identity::sync_instructions()?;
        }
        Ok(0)
    })
}

/// 发起调用的那个任务（正在跑的，或者是这个 ident 对应的）。
fn caller(ident: &Arc<TaskIdent>) -> Option<Arc<Task>> {
    use crate::basis::room::conductor::current;
    use crate::basis::room::scheduler::roster::muster;
    current()
        .running_task()
        .or_else(|| muster(ident.id).and_then(|task| task.upgrade()))
}

/// 来源侧的失败码折成地址空间同轴的那一条。
fn source_fail(error: kernel_api::abi::call::pend::PendFail) -> CellFail {
    match error {
        kernel_api::abi::call::pend::PendFail::OoM => CellFail::OoM,
        kernel_api::abi::call::pend::PendFail::Busy => CellFail::Busy,
        kernel_api::abi::call::pend::PendFail::NotAligned => CellFail::NotAligned,
        _ => CellFail::Denied,
    }
}

/// 核验调用者表内活的 Retain 控制授权及内核铸造的保留对象身份。
fn require_retention(
    task: &Arc<Task>,
    token: kernel_api::model::unit::ControlId,
) -> Result<(), CellFail> {
    if task
        .administration
        .matches(token, kernel_api::model::unit::ControlKind::Retain)
    {
        Ok(())
    } else {
        Err(CellFail::Denied)
    }
}

/// 登记一份来源持有，返回它在**调用任务**引用表里的句柄。
///
/// 新的 `Allocation` 建立来源持有与一份一次性创建资格；已有引用的子范围与它
/// **共享**同一份来源授权（同一来源、同一上限、同一撤销依赖），不复制资格，
/// 也不因 VA 可读就授权。
///
/// # 窗口守在哪几条路上
///
/// **有效窗口属于那一枚分支，不属于任务**：同一任务手里可以有同源的几枚 token，
/// 每一枚各自有自己那一段（权限与窗口都是各自那份，不是并成一张"本任务的图"）。
/// 故取值要按**那一枚**分支算：
/// - `AcquireSource::Source` 解析出源坐标后当场核窗口（越窗 `Denied`）；
/// - `AcquireSource::Reference` **不另核**：引用是早先经 `Source` 那条路建的，
///   它的范围建立时就已落在**建它的那一枚**分支的窗口里，且 `hold.branch()` 就是
///   那一枚（子段只在这个既有范围里取）。同任务另一枚更宽的 token 换不掉这个边界。
///   将来若添直接建引用的入口，它必须同样从一枚按窗口筛过的分支上建，这条不变量才继续成立。
fn acquire(
    source: AcquireSource,
    retention: kernel_api::model::unit::ControlId,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    let task = caller(ident).ok_or(CellFail::Denied)?;
    let _serial = task.pend.lock();
    // 登记、准入复核与写入同在一个 `commit` 段：独立持有的登记位与物理保留一起备好，
    // 这一段之后不再有可失败的准备，也不会在判完之后被人插进一条新占用。
    let _commit = crate::basis::commit();
    let (authorization, offset, count, ceiling, retained, branch) = match source {
        AcquireSource::Allocation(allocation) => {
            // 新建来源本身就没有旧授权可挂，独立保留授权对它无意义。
            if retention != kernel_api::model::unit::ControlId::NONE
                || allocation.align != PAGE_SIZE
            {
                return Err(CellFail::NotAligned);
            }
            let size = allocation.bytes().ok_or(CellFail::Denied)?;
            let pages = size / PAGE_SIZE;
            let count = PageCount::new(pages).ok_or(CellFail::NotAligned)?;
            let range =
                Flat::acquire(ident.team.cells.clone(), Allocation::new(pages, PAGE_SIZE)?)?
                    .whole();
            let authorization =
                SourceAuthorization::new(range, Access::all(), ident.id).map_err(source_fail)?;
            let ceiling = authorization.ceiling();
            (authorization, PageOffset::ZERO, count, ceiling, None, None)
        }
        AcquireSource::Reference(window) => {
            let hold = task
                .pend
                .references
                .get(window.reference.get())
                .ok_or(CellFail::Denied)?;
            let count = PageCount::new(window.count).ok_or(CellFail::NotAligned)?;
            let range = hold
                .resolve()
                .ok_or(CellFail::Denied)?
                .range(PageOffset::new(window.offset), count)
                .map_err(|_| CellFail::NoRegion)?;
            let (parent, _) = hold.window();
            let offset = PageOffset::new(
                parent
                    .get()
                    .checked_add(window.offset)
                    .ok_or(CellFail::NotAligned)?,
            );
            // 独立持有：此刻把这段范围从当前授权上解下来自己强持，
            // 于是它跨 Pend 撤销继续持同一批帧；不带保留的仍随授权生死。
            // 从持有人自己那条引用上取的，不再把这一段当别人的占用；这一条只核子段
            // （见 `check_use` 的说明：取得又一份持有不是发表）。
            hold.source().ensure_internal().map_err(source_fail)?;
            // 它在来源记录上的登记与物理保留一起生：登记位先备、写入不再失败。
            let retained = if retention == kernel_api::model::unit::ControlId::NONE {
                None
            } else {
                require_retention(&task, retention)?;
                let source = hold.source();
                source.reserve_retention().map_err(source_fail)?;
                let window = Geometry::new(offset, count).map_err(source_fail)?;
                Some(IndependentHold::new(
                    source,
                    window,
                    range.reside()?,
                    ident.id,
                ))
            };
            (
                hold.source().clone(),
                offset,
                count,
                hold.ceiling(),
                retained,
                hold.branch()
                    .map(|(token, authority)| (token, authority.clone())),
            )
        }
        AcquireSource::Source {
            token,
            offset,
            count,
        } => {
            // 从 Share 的某一枚 PendEntry 取得：撤销边与上限按**这一枚**的子 Pend 算，
            // 不因同 Flat、共享根资格就并到根上去。
            let pend = crate::basis::pend::locate(&task, token).ok_or(CellFail::Denied)?;
            if !pend.alive() {
                return Err(CellFail::Denied);
            }
            let meta = pend.view().share_view().ok_or(CellFail::Denied)?;
            if !pend.modifiable().allows(Modifiable::No) {
                return Err(CellFail::Denied);
            }
            let authorization = meta.authorization().ok_or(CellFail::Denied)?;
            let ceiling = authorization.ceiling();
            // 分支就是调用者手里这一枚 PendEntry 的 token 与它的 Pend。
            let authority = pend.authority().clone();
            let (offset, count) = self::window(&authorization, offset, count)?;
            // 这一枚分支若有有效窗口，请求的那一段必须落在它里面。
            // 两边都是**相对来源起点的绝对页偏移**，故嵌套派生不会叠加两层偏移。
            if let Some((window_offset, window_count)) = authority.window() {
                let end = window_offset
                    .get()
                    .checked_add(window_count.get())
                    .ok_or(CellFail::NotAligned)?;
                let requested_end = offset
                    .get()
                    .checked_add(count.get())
                    .ok_or(CellFail::NotAligned)?;
                if offset.get() < window_offset.get() || requested_end > end {
                    return Err(CellFail::Denied);
                }
            }
            // 从分支取引用不是发表：只核子段，不再落一份独立持有也不交出这一段。
            authorization.ensure_internal().map_err(source_fail)?;
            let retained = if retention == kernel_api::model::unit::ControlId::NONE {
                None
            } else {
                require_retention(&task, retention)?;
                authorization.reserve_retention().map_err(source_fail)?;
                let window = Geometry::new(offset, count).map_err(source_fail)?;
                let residency = meta
                    .source()
                    .range(offset, count)
                    .map_err(|_| CellFail::NoRegion)?
                    .reside()?;
                Some(IndependentHold::new(
                    &authorization,
                    window,
                    residency,
                    ident.id,
                ))
            };
            (
                authorization,
                offset,
                count,
                ceiling,
                retained,
                Some((token, authority)),
            )
        }
        AcquireSource::Mapping {
            team,
            mapping,
            offset,
            count,
        } => with_mapping(team, mapping, ident, |_target, entry| {
            // 从已授权的关系取得它的来源范围；域、所属关系与来源授权都要核，
            // MappingId 或可读 VA 本身不是凭证。
            let mapping = entry.mapping().ok_or(CellFail::Denied)?.clone();
            let authorization = entry.source.clone();
            let count = PageCount::new(count).ok_or(CellFail::NotAligned)?;
            let source = mapping
                .source()
                .range(PageOffset::new(offset), count)
                .map_err(|_| CellFail::NoRegion)?;
            let base = authorization.index().ok_or(CellFail::Denied)?;
            let offset = PageOffset::new(
                source
                    .index()
                    .get()
                    .checked_sub(base.get())
                    .ok_or(CellFail::NotAligned)?,
            );
            let (offset, count) = self::window(&authorization, offset.get(), count.get())?;
            let ceiling = entry
                .mapping()
                .map_or(Access::all(), |_| authorization.ceiling());
            let branch = if let Some((meta, root_token, _)) = authorization.root() {
                let (_, branch) = entry.control.as_ref().ok_or(CellFail::Denied)?.control();
                let token = branch.unwrap_or(root_token);
                let pend = crate::basis::pend::locate(&task, token).ok_or(CellFail::Denied)?;
                let authority = meta.authority(token).ok_or(CellFail::Denied)?;
                if !pend.alive()
                    || !pend.modifiable().allows(Modifiable::No)
                    || !Arc::ptr_eq(pend.authority(), &authority)
                {
                    return Err(CellFail::Denied);
                }
                Some((token, authority))
            } else {
                if !authorization.private_owner(ident.id) {
                    return Err(CellFail::Denied);
                }
                None
            };
            // 从已授权关系取引用也不是发表：只核子段。
            authorization.ensure_internal().map_err(source_fail)?;
            let retained = if retention == kernel_api::model::unit::ControlId::NONE {
                None
            } else {
                require_retention(&task, retention)?;
                authorization.reserve_retention().map_err(source_fail)?;
                let window = Geometry::new(offset, count).map_err(source_fail)?;
                Some(IndependentHold::new(
                    &authorization,
                    window,
                    source.reside()?,
                    ident.id,
                ))
            };
            Ok((authorization, offset, count, ceiling, retained, branch))
        })?,
    };
    task.pend
        .references
        .insert(authorization, offset, count, ceiling, retained, branch)
        .map_err(source_fail)
}

/// 把线外的页窗口换成相对来源整段的坐标，并核它落在来源内。
fn window(
    authorization: &Arc<SourceAuthorization>,
    offset: usize,
    count: usize,
) -> Result<(PageOffset, PageCount), CellFail> {
    let count = PageCount::new(count).ok_or(CellFail::NotAligned)?;
    let offset = PageOffset::new(offset);
    if authorization
        .full_range()
        .is_none_or(|range| range.range(offset, count).is_err())
    {
        return Err(CellFail::NoRegion);
    }
    Ok((offset, count))
}

/// 解除这一份引用：只摘掉本表里这一格，不 Detach、不等其他引用、不分配。
/// 授权已经失效也不妨碍清掉自己的句柄。
///
/// 独立保留随这一份引用结束：那是一个显式的“不再物理持有这一段”事件，
/// 所以在这里重试父侧已可证明结束的分配槽；不可证明的照旧占着。
fn release(
    reference: kernel_api::model::cell::FlatReference,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    let task = caller(ident).ok_or(CellFail::Denied)?;
    let _serial = task.pend.lock();
    let hold = task
        .pend
        .references
        .remove(reference.get())
        .ok_or(CellFail::Denied)?;
    drop(hold);
    Ok(0)
}

/// 移交那一域的**在途键**：高一位是域标记，低段是自己那一枚单调号。
///
/// 在途表按**键**认条目，而键来自两个分配器（`Attach` 的占用号那一域与移交这一域），
/// 各自的分配器只保证**域内**唯一、数值上可以撞：所以键自带域，撤登记时才不会把
/// 同号的另一域条目删掉（反向也一样）。号到符号上界就到：明确失败，不 panic。
fn next_flight_key() -> Result<usize, CellFail> {
    const FLIGHT_DOMAIN: usize = 1 << (usize::BITS - 1);
    static NEXT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(1);
    crate::machine::sync::next_id(&NEXT)
        .map(|id| FLIGHT_DOMAIN | id)
        .ok_or(CellFail::OoM)
}

/// `Transfer`：把源窗口里的一页承载交进目标窗口的空槽。
///
/// 两个页号各相对**自己那份引用**的窗口：这里向引用解析窗口，页号换来源绝对
/// 坐标只在 `Transfer::new` 里做一次，不叠加第二次偏移。
///
/// 两端的**管理授权**都在同一个 `caller.pend` 下核，且这个 pend 一直持到
/// 执行结算或回退完毕：授权失效的每一处写点（doom 先清引用再关控制、
/// cull/release/revoke、accord、Pend 收窄、入口 Seal）都要同一把 pend 串行，
/// 所以迁移期间来源不会在脚下被撤走。这里不取另一个 Task 的 pend。
///
/// **提交段只盖准入与在途登记**：两端各自那一页在同一 `commit` 段里核完新使用
/// 条件后，当场把“这一段已核、不得插进别的发表”登在各自来源上（带窗口的在途）。
/// 段一放就走执行——执行里有等别的 hart 的窗口（撤页、确认旧 TLB、夹具的在途探针），
/// 不能把全局提交锁带进去；执行期靠那两条在途登记守住已核的移动范围，
/// 结算后按同一笔键一次撤完（两域同号互不干涉，见 `next_flight_key`）。
/// 旧引用因此不能把已分出的子段或别人的独立持有移来移去，也不会把对端饿死在锁上。
///
/// `ShareOperation` 不是授权，这里不取它：槽的独占与逐页撤销的串行由事务与各
/// Nest 自己的临界区负责，另取一把 Share 租约只会把“同根两端”变成第二个失败面。
fn transfer(
    from: kernel_api::model::cell::FlatReference,
    from_page: usize,
    to: kernel_api::model::cell::FlatReference,
    to_page: usize,
    change: usize,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    let caller = caller(ident).ok_or(CellFail::Denied)?;
    let serial = caller.pend.lock();
    if !pend::live(&caller) {
        return Err(CellFail::Denied);
    }
    let source = endpoint(&caller, from)?;
    let target = endpoint(&caller, to)?;
    let transfer = Transfer::new(
        source,
        PageOffset::new(from_page),
        target,
        PageOffset::new(to_page),
    )
    .map_err(CellFail::from)?;
    // 两端各自的引用与这一页在它窗口里的相对位置：可失败的准备全在提交段之前。
    let permit = if change == 0 {
        None
    } else {
        Some(crate::basis::cell::changes::get(ident.id, change)?)
    };
    let flight = next_flight_key()?;
    let mut ends: Vec<(Arc<Hold>, Arc<Geometry>)> = Vec::new();
    ends.try_reserve_exact(2).map_err(|_| CellFail::OoM)?;
    for (reference, page) in [(from, from_page), (to, to_page)] {
        let hold = caller
            .pend
            .references
            .get(reference.get())
            .ok_or(CellFail::Denied)?;
        let offset = PageOffset::new(
            hold.window()
                .0
                .get()
                .checked_add(page)
                .ok_or(CellFail::NotAligned)?,
        );
        let count = PageCount::new(1).ok_or(CellFail::NotAligned)?;
        let window = Geometry::new(offset, count).map_err(source_fail)?;
        ends.push((hold, window));
    }
    // 准入复核与在途登记同一个段：先把两端都核完（两边互相不干扰，同一页那种
    // 自相矛盾交给移交自己的事务去拒），再落两端的在途窗口。执行期不再持 Unit，
    // 别的发表要么在段前看见占用被拒，要么在段后看见在途窗口被拒。
    {
        let _commit = crate::basis::commit();
        for (hold, window) in &ends {
            let (offset, count) = window.window();
            hold.source()
                .check_publish(offset, count, hold.retention())
                .map_err(source_fail)?;
        }
        let mut at = 0;
        while at < ends.len() {
            let (hold, window) = &ends[at];
            if let Err(error) = hold.source().enter_publish_range(flight, window.clone()) {
                for (held, _) in &ends[..at] {
                    held.source().leave_publish(flight);
                }
                return Err(source_fail(error));
            }
            at += 1;
        }
    }
    let outcome = transfer.execute(permit.as_ref());
    // 已经结算或回退完毕：同一笔键把两端的在途登记一次撤完，
    // 再放掉 pend，最后才取执行结果——中途不用 `?` 抖掉失败回退的通知。
    for (hold, _) in &ends {
        hold.source().leave_publish(flight);
    }
    drop(serial);
    wake(outcome.affected);
    outcome.result.map(|()| 0).map_err(CellFail::from)
}

/// 一端：向这份引用解析出它的窗口，同时当场核对调用者是不是那份来源的管理者。
///
/// 引用只在自己那张表里解析得动（伪号或已释放的旧号当场 `Denied`）；
/// 解析不出窗口（来源已被根 Pend 接管并撤销）也是 `Denied`。
fn endpoint(
    caller: &Arc<Task>,
    reference: kernel_api::model::cell::FlatReference,
) -> Result<FlatRange, CellFail> {
    let hold = caller
        .pend
        .references
        .get(reference.get())
        .ok_or(CellFail::Denied)?;
    if !hold.source().manages(caller) {
        return Err(CellFail::Denied);
    }
    hold.resolve().ok_or(CellFail::Denied)
}

/// 这次移交动过的关系的**唯一生产消费者**：把每一条变成它自己的等待位放行。
///
/// 顺序是合同的一部分：升级 `Weak` 取到 `(ASID, 关系身份)`，先放掉 Mapping/Nest
/// 强引用，再 `signal` 整链——通知不在任何关系强引用下进出。清单里已经消失的
/// 关系没有等待者，直接跳过；`signal` 非 sticky，没有等待者时就是空放行。
fn wake(affected: Vec<Weak<Mapping>>) {
    for relation in affected {
        let Some(mapping) = relation.upgrade() else {
            continue;
        };
        let key = mapping.wake_key();
        drop(mapping);
        let Some((space, mapping)) = key else {
            continue;
        };
        sign::signal(WakeKey::Mapping { space, mapping });
    }
}

/// 地址空间失败折成线上码：`Denied` 兼收 `Pinned`、`NotMapped` 与 `DramOverlap`。
impl From<NestError> for CellFail {
    fn from(error: NestError) -> Self {
        match error {
            NestError::OutOfMemory => CellFail::OoM,
            NestError::NotAligned => CellFail::NotAligned,
            NestError::NoRegion => CellFail::NoRegion,
            NestError::AlreadyMapped => CellFail::AlreadyMapped,
            NestError::WidenDenied => CellFail::WidenDenied,
            NestError::Shootdown => CellFail::Busy,
            NestError::Pinned => CellFail::Denied,
            // 承载已移出：折成既有的“没有所属区域”，不给新线上码，
            // 也不当可无限重试的 Busy。
            NestError::Missing => CellFail::NoRegion,
            NestError::Busy => CellFail::Busy,
            NestError::NotMapped | NestError::DramOverlap => CellFail::Denied,
        }
    }
}

/// 来源失败与地址空间同轴折叠。
impl From<FlatError> for CellFail {
    fn from(error: FlatError) -> Self {
        NestError::from(error).into()
    }
}

/// 移交失败折成线上码：越界是“不在任何已声明的映射里”，分配耗尽与可重试的
/// 未确认同步各自同轴；三格新码只描述移交自己的两种空槽与永久拒结。
impl From<TransferError> for CellFail {
    fn from(error: TransferError) -> Self {
        match error {
            TransferError::OutOfMemory => CellFail::OoM,
            TransferError::OutOfRange => CellFail::NoRegion,
            TransferError::SourceEmpty => CellFail::SourceEmpty,
            TransferError::DestinationOccupied => CellFail::DestinationOccupied,
            TransferError::NotTransferable => CellFail::NotTransferable,
            TransferError::Busy => CellFail::Busy,
        }
    }
}

fn attach_target(
    target: kernel_api::model::cell::MappingTarget,
    request: kernel_api::address::VirtAddr,
    ident: &Arc<TaskIdent>,
) -> Result<(usize, usize), CellFail> {
    use kernel_api::model::cell::{Attachment, MappingControl, MappingTarget, PlacementAt};
    let spec = Attachment::from_bytes(&read_snapshot::<{ Attachment::WIDTH }>(
        &ident.team.space,
        request,
    )?)
    .ok_or(CellFail::NotAligned)?;
    let token = match target {
        MappingTarget::Cpu(team) => {
            if spec.change != 0 {
                return Err(CellFail::Denied);
            }
            return attach(
                spec.range,
                team,
                spec.placement,
                spec.access,
                spec.control,
                ident,
            );
        }
        MappingTarget::Io(token) => token,
    };
    if spec.control != MappingControl::Caller
        || !matches!(spec.access, 2 | 6)
        || spec.range.offset != 0
        || spec.range.count != 1
    {
        return Err(CellFail::Denied);
    }
    let PlacementAt::Exact(address) = spec.placement.at() else {
        return Err(CellFail::Denied);
    };
    let range = spec.range;
    let actor = caller(ident).ok_or(CellFail::Denied)?;
    let _serial = actor.pend.lock();
    let _commit = crate::basis::commit();
    if !pend::live(&actor) {
        return Err(CellFail::Denied);
    }
    let hold = actor
        .pend
        .references
        .get(range.reference.get())
        .ok_or(CellFail::Denied)?;
    let id = crate::basis::unit::space::external::attach(
        &actor,
        token,
        &hold,
        address.get(),
        spec.access == 6,
        if spec.change == 0 {
            None
        } else {
            Some(crate::basis::cell::changes::get(ident.id, spec.change)?)
        }
        .as_ref(),
    )?;
    Ok((id, address.get()))
}

fn begin(
    source: kernel_api::model::cell::FlatReference,
    page: usize,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    let actor = caller(ident).ok_or(CellFail::Denied)?;
    let _serial = actor.pend.lock();
    if !pend::live(&actor) {
        return Err(CellFail::Denied);
    }
    let cell = crate::basis::cell::Cell::Flat(endpoint(&actor, source)?);
    let page = cell.page(page).map_err(|e| {
        if e == NestError::Missing {
            CellFail::SourceEmpty
        } else {
            e.into()
        }
    })?;
    crate::basis::cell::changes::begin(ident.id, &page)
}

fn end(change: usize, ident: &Arc<TaskIdent>) -> Result<usize, CellFail> {
    let actor = caller(ident).ok_or(CellFail::Denied)?;
    let serial = actor.pend.lock();
    if !pend::live(&actor) {
        return Err(CellFail::Denied);
    }
    crate::basis::cell::changes::end(ident.id, change)?;
    drop(serial);
    // Faults waiting on a changed source re-evaluate their retained predicates.
    crate::basis::cell::changes::maintain();
    Ok(0)
}

fn push(
    source: kernel_api::model::cell::FlatReference,
    page: usize,
    at: kernel_api::address::VirtAddr,
    change: usize,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    use kernel_api::model::cell::Position;
    let destination = Position::from_bytes(&read_snapshot::<{ Position::WIDTH }>(
        &ident.team.space,
        at,
    )?)
    .ok_or(CellFail::NotAligned)?;
    match destination {
        Position::Slot {
            reference,
            page: target_page,
        } => transfer(source, page, reference, target_page, change, ident),
        Position::Memo {
            team,
            mapping,
            page: target_page,
        } => {
            with_mapping(team, mapping, ident, |target, entry| {
                let actor = caller(ident).ok_or(CellFail::Denied)?;
                let source_range = endpoint(&actor, source)?
                    .range(PageOffset::new(page), PageCount::new(1).unwrap())
                    .map_err(NestError::from)?;
                let selection = selection(
                    entry,
                    kernel_api::model::cell::MappingSelection::Pages {
                        offset: target_page,
                        count: 1,
                    },
                )?;
                // Preserve the source authority attached to this Mapping. Changing
                // its provider requires a new authorized binding, not a Page ID.
                if !selection.source()?.same_range(&source_range) {
                    return Err(CellFail::Denied);
                }
                let page = crate::basis::cell::Cell::Flat(source_range)
                    .page(0)
                    .map_err(|e| {
                        if e == NestError::Missing {
                            CellFail::SourceEmpty
                        } else {
                            e.into()
                        }
                    })?;
                let permit = if change == 0 {
                    page.begin().map_err(NestError::from)?
                } else {
                    crate::basis::cell::changes::get(ident.id, change)?
                };
                if !Arc::ptr_eq(permit.page(), &page) {
                    return Err(CellFail::Denied);
                }
                let source = &entry.source;
                let root = source.root();
                let meta = root.as_ref().map(|(meta, _, _)| meta);
                let _operation = meta
                    .map(|meta| ShareOperation::acquire(meta).ok_or(CellFail::Busy))
                    .transpose()?;
                let branch = entry
                    .control
                    .as_ref()
                    .and_then(|control| control.control().1);
                let authority = meta
                    .zip(branch)
                    .and_then(|(meta, branch)| meta.authority(branch));
                if branch.is_some() && authority.is_none() {
                    return Err(CellFail::Denied);
                }
                let need = selection
                    .mapping()
                    .state
                    .lock()
                    .range_at(selection.window().0)
                    .ok_or(CellFail::NoRegion)?
                    .ceiling;
                let flight = next_flight_key()?;
                source
                    .enter_publish(flight, authority.as_deref(), need)
                    .map_err(source_fail)?;
                let result = target.space.nest.push_page(
                    selection.mapping(),
                    KVirt::new(selection.window().0),
                    &page,
                    &permit,
                );
                let valid = source.branch_check(authority.as_deref(), need) && pend::live(&actor);
                let rollback = if !valid {
                    target.space.nest.unpublish_page(
                        selection.mapping(),
                        KVirt::new(selection.window().0),
                        &page,
                    )
                } else {
                    Ok(())
                };
                source.leave_publish(flight);
                rollback?;
                if !valid {
                    return Err(CellFail::Denied);
                }
                result?;
                if need.contains(Access::EXECUTE) {
                    identity::sync_instructions()?;
                }
                if let Some((space, mapping)) = selection.mapping().wake_key() {
                    sign::signal(WakeKey::Mapping { space, mapping });
                }
                Ok(0)
            })
        }
    }
}

fn pull(
    at: kernel_api::address::VirtAddr,
    destination: kernel_api::model::cell::FlatReference,
    page: usize,
    change: usize,
    ident: &Arc<TaskIdent>,
) -> Result<usize, CellFail> {
    use kernel_api::model::cell::Position;
    let source = Position::from_bytes(&read_snapshot::<{ Position::WIDTH }>(
        &ident.team.space,
        at,
    )?)
    .ok_or(CellFail::NotAligned)?;
    match source {
        Position::Slot {
            reference,
            page: source_page,
        } => transfer(reference, source_page, destination, page, change, ident),
        Position::Memo {
            team,
            mapping,
            page: source_page,
        } => {
            if destination != kernel_api::model::cell::FlatReference::NONE || page != 0 {
                return Err(CellFail::Denied);
            }
            with_mapping(team, mapping, ident, |target, entry| {
                let selection = selection(
                    entry,
                    kernel_api::model::cell::MappingSelection::Pages {
                        offset: source_page,
                        count: 1,
                    },
                )?;
                target.space.nest.retry_invalidation()?;
                let page = crate::basis::cell::Cell::Nest(selection.clone())
                    .page(0)
                    .map_err(|e| {
                        if e == NestError::Missing {
                            CellFail::SourceEmpty
                        } else {
                            e.into()
                        }
                    })?;
                let permit = if change == 0 {
                    page.begin().map_err(NestError::from)?
                } else {
                    crate::basis::cell::changes::get(ident.id, change)?
                };
                if !Arc::ptr_eq(permit.page(), &page) {
                    return Err(CellFail::Denied);
                }
                target.space.nest.unpublish_page(
                    selection.mapping(),
                    KVirt::new(selection.window().0),
                    &page,
                )?;
                Ok(0)
            })
        }
    }
}
