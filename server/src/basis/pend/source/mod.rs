//! 一份**来源授权**：某个来源下所有普通引用与已建关系共享的唯一记录。
//!
//! 它住在工作层（与引用表、创建资格、事务同处），由执行入口与 Share 创建共同使用；
//! 核心 `Mapping` 不回指它。私有根 → 根 Pend 的交接只改这里的 `phase` 一处权威，
//! 不为每份引用复制一份状态再扫表更新。资格只属于这份记录，
//! 普通引用的有无靠显式语义判断，不数 `Arc` 强引用。
//!
//! 名字带 `Authorization` 是为了与 Flat 的“来源”（`Flat` / `FlatRange`）分开：
//! 这一份说的是**授权关系**，不是物理来源。

use alloc::sync::Arc;
use alloc::sync::Weak;
use alloc::vec::Vec;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU8;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use crate::basis::flat::FlatRange;
use crate::basis::nest::Access;
use crate::basis::nest::Nest;
use crate::basis::nest::mapping::MappingBinding;
use crate::basis::pend::Pend;
use crate::basis::view::share::ShareView;
use crate::machine::sync::SpinLock;
use kernel_api::abi::call::pend::PendFail;
use kernel_api::address::PageCount;
use kernel_api::address::PageIndex;
use kernel_api::address::PageOffset;
use kernel_api::model::identity::PendId;
use kernel_api::model::identity::TaskId;
use kernel_api::model::modifiable::Modifiable;

mod external;
mod geometry;
mod hold;

pub(crate) use external::ExternalBorrow;
pub(crate) use geometry::Geometry;
pub(crate) use hold::{Hold, HoldTable};

// 一份来源授权当前归谁管：私有根，还是已经交给根 Pend。
//
// 两个集合分得很清楚：私有阶段已建的关系记在自己这边；交给 Share 之后
// 关系由那个 token 下的登记管，撤销随之关闭。

#[derive(Clone)]
enum Phase {
    /// 还没有 Share 接手。
    Private,
    /// 已由根 Pend 接手：撤销依赖那枚 Pend，后续关系交给 Share 管。
    Root {
        meta: Weak<ShareView>,
        token: PendId,
        authority: Arc<Pend>,
    },
}

const AVAILABLE: u8 = 0;
const CLAIMED: u8 = 1;
const CONSUMED: u8 = 2;

struct Creation {
    state: AtomicU8,
}

impl Creation {
    fn new() -> Self {
        Self {
            state: AtomicU8::new(AVAILABLE),
        }
    }

    fn claim(&self) -> Result<(), kernel_api::abi::call::pend::PendFail> {
        match self
            .state
            .compare_exchange(AVAILABLE, CLAIMED, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => Ok(()),
            Err(CLAIMED) => Err(kernel_api::abi::call::pend::PendFail::Busy),
            Err(_) => Err(kernel_api::abi::call::pend::PendFail::Denied),
        }
    }

    fn consume(&self) {
        self.state.store(CONSUMED, Ordering::Release);
    }

    fn release(&self) {
        let _ =
            self.state
                .compare_exchange(CLAIMED, AVAILABLE, Ordering::AcqRel, Ordering::Acquire);
    }
}

pub(crate) struct SourceAuthorization {
    /// 私有阶段由它自己强持整段来源；交给根 Pend 之后这里变空，
    /// 真实范围改由 Share 持有——普通引用因此不会在撤销后继续 pin 住帧。
    range: SpinLock<Option<FlatRange>>,
    ceiling: Access,
    owner: TaskId,
    qualification: Creation,
    external: AtomicBool,
    /// 还活着的普通引用份数（成员资格，不是 `Arc` 强计数）。
    holds: AtomicUsize,
    /// **在途发布**的键：闸门下登记，撤销方看到它就不结清。
    ///
    /// 键来自两个分配器（关系号那一域与移交那一域），各自只保证**域内**唯一；
    /// 带窗口的那一档（移交）还把“这一段已经核过、别的发表不得插进来”一并记下。
    /// 两张域靠键自带的那一位分开，故同号不会互删。
    inflight: SpinLock<Vec<(usize, Option<Arc<Geometry>>)>>,
    /// 发布与撤销的共同闸门。
    revoked: SpinLock<bool>,
    /// 未结 CPU 关系：包含显式 Attach 控制与资源 Open 映射。
    ///
    /// 成员与 Registry 项是同一个 `Arc<MappingBinding>`，所以“还欠着谁”只有这一份
    /// 事实：只有显式结清（`Nest::retire_mapping` / `reap_mappings`）才撒下成员，
    /// 不靠 `Weak` 升不上或 Drop 替它做主。撤销方也按它把名下关系收回去关。
    bindings: SpinLock<Vec<Arc<MappingBinding>>>,
    phase: SpinLock<Phase>,
}

/// 私有根在根 Pend 接手前的权限幅面（上限仍由 `ceiling` 收）。
fn private_access() -> Modifiable {
    Modifiable::Yes
}

/// 活跃读权可以读取；写访问另外要求 Modifiable。
fn covers(modifiable: Modifiable, access: Access) -> bool {
    modifiable.allows(Modifiable::No)
        && (!access.contains(Access::WRITE) || modifiable == Modifiable::Yes)
}

impl SourceAuthorization {
    pub(crate) fn new(
        range: FlatRange,
        ceiling: Access,
        owner: TaskId,
    ) -> Result<Arc<Self>, PendFail> {
        Arc::try_new(Self {
            range: SpinLock::new(Some(range)),
            ceiling,
            owner,
            qualification: Creation::new(),
            external: AtomicBool::new(false),
            holds: AtomicUsize::new(0),
            inflight: SpinLock::new(Vec::new()),
            revoked: SpinLock::new(false),
            bindings: SpinLock::new(Vec::new()),
            phase: SpinLock::new(Phase::Private),
        })
        .map_err(|_| PendFail::OoM)
    }

    /// Under the shared work commit boundary: no old CPU publication may remain,
    /// and every future source publication observes this distinct borrow state.
    pub(super) fn begin_external(&self, task: TaskId) -> Result<(), PendFail> {
        #[cfg(debug_assertions)]
        crate::basis::assert_held();
        if !self.private_owner(task) {
            return Err(PendFail::Denied);
        }
        let gate = self.revoked.lock();
        if *gate
            || self.bindings.lock().iter().any(|binding| {
                binding.nest().upgrade().is_none_or(|space| {
                    space.has_pending_invalidation()
                        || space.mappings.registered(binding.id()).is_none_or(|entry| {
                            entry
                                .mapping()
                                .is_none_or(|mapping| !mapping.state.lock().published.is_empty())
                        })
                })
            })
            || !self.inflight.lock().is_empty()
        {
            return Err(PendFail::Busy);
        }
        self.external
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| PendFail::Busy)
    }
    pub(super) fn end_external(&self) {
        #[cfg(debug_assertions)]
        crate::basis::assert_held();
        self.external.store(false, Ordering::Release);
    }

    /// 整段来源范围；已交给根 Pend 就是 `None`（真实范围在 Share 手里）。
    pub(crate) fn own_range(&self) -> Option<FlatRange> {
        self.range.lock().clone()
    }

    pub(crate) fn ceiling(&self) -> Access {
        self.ceiling
    }

    pub(crate) fn private_owner(&self, task: TaskId) -> bool {
        !self.external.load(Ordering::Acquire)
            && self.owner == task
            && matches!(&*self.phase.lock(), Phase::Private)
    }

    /// 相对整段来源的页窗口解析成可用范围。
    ///
    /// 私有阶段用自己的强持范围；已接管就向 Share 要，且要求那枚 Pend 仍活——
    /// 于是撤销后的普通引用解析不到范围，自然既不挂载也不 pin 住来源。
    pub(crate) fn window(&self, offset: PageOffset, count: PageCount) -> Option<FlatRange> {
        if let Some(range) = self.own_range() {
            return range.range(offset, count).ok();
        }
        let (meta, _, authority) = self.root()?;
        self.modifiable()?;
        if !authority.contains(offset, count) {
            return None;
        }
        meta.source().range(offset, count).ok()
    }

    /// 整段来源范围（创建事务要用它建 Share / 判完整）。
    pub(crate) fn full_range(&self) -> Option<FlatRange> {
        if let Some(range) = self.own_range() {
            return Some(range);
        }
        self.root().map(|(meta, _, _)| meta.source())
    }

    /// 整段来源的页起点（子范围 Acquire 要把视图的坐标换成相对本来源的坐标）。
    pub(crate) fn index(&self) -> Option<PageIndex> {
        self.full_range().map(|range| range.index())
    }

    /// 该来源当前的权限幅面；`None` = 已撤销。
    pub(crate) fn modifiable(&self) -> Option<Modifiable> {
        let _gate = self.revoked.lock();
        self.permission_ungated()
    }

    /// 闸门已持有时读当前权限幅面（不再回取闸门）。
    fn permission_ungated(&self) -> Option<Modifiable> {
        match &*self.phase.lock() {
            Phase::Private => Some(private_access()),
            Phase::Root { authority, .. } => authority.modifiable(),
        }
    }

    /// 同一条提交边界，但上限取自**这次引用所在的那条分支**（子 PendEntry 的 Pend）。
    ///
    /// 不同 Pend 分支不是同一事实：子 PendEntry 收窄/撤销后，它的分支不得再挂可写关系，
    /// 而另一条分支不受影响。
    pub(crate) fn branch_check(&self, branch: Option<&Pend>, access: Access) -> bool {
        let gate = self.revoked.lock();
        !*gate && self.branch_permits(branch, access)
    }

    fn branch_permits(&self, branch: Option<&Pend>, access: Access) -> bool {
        if self.external.load(Ordering::Acquire) {
            return false;
        }
        // 先把相位拷出来放锁，再升弱引用：临时 `Arc` 可能是最后一份强持，
        // 它的析构会走到 `retire_mapping` / `settle` 再回本文件取锁。同 `manages`。
        let phase = Phase::clone(&self.phase.lock());
        let alive = match phase {
            Phase::Private => true,
            Phase::Root { meta, .. } => meta.upgrade().is_some_and(|meta| meta.alive()),
        };
        if !alive {
            return false;
        }
        let modifiable = match branch {
            Some(authority) => authority.modifiable(),
            None => self.permission_ungated(),
        };
        modifiable.is_some_and(|p| covers(p, access))
    }

    /// 在途登记（闸门下的短临界区）：还没被撤销才收下这个键。
    pub(crate) fn enter_publish(
        &self,
        id: usize,
        branch: Option<&Pend>,
        access: Access,
    ) -> Result<(), PendFail> {
        self.enter_flight(id, None, branch, access)
    }

    /// 同一条在途登记，但这次发表**占住了**这一段窗口：
    ///
    /// 别的发表（派生 / Open / Accord / Attach）在这笔在途结束前不得插进这一段
    /// （见 `check_publish`）；撤销方与不带窗口的那一档一样看待它。
    pub(crate) fn enter_publish_range(
        &self,
        id: usize,
        window: Arc<Geometry>,
    ) -> Result<(), PendFail> {
        self.enter_flight(id, Some(window), None, Access::READ)
    }

    fn enter_flight(
        &self,
        id: usize,
        window: Option<Arc<Geometry>>,
        branch: Option<&Pend>,
        access: Access,
    ) -> Result<(), PendFail> {
        let gate = self.revoked.lock();
        if *gate || !self.branch_permits(branch, access) {
            return Err(PendFail::Denied);
        }
        let mut inflight = self.inflight.lock();
        inflight.try_reserve(1).map_err(|_| PendFail::OoM)?;
        inflight.push((id, window));
        Ok(())
    }

    pub(crate) fn begin_withdraw(
        &self,
    ) -> Result<crate::machine::sync::spin::SpinLockGuard<'_, bool>, PendFail> {
        let gate = self.revoked.lock();
        if !self.inflight.lock().is_empty() {
            return Err(PendFail::Busy);
        }
        Ok(gate)
    }

    /// 在途登记失效（提交或回滚都走这一步）：同一笔键的两条一次撤完。
    pub(crate) fn leave_publish(self: &Arc<Self>, id: usize) {
        self.inflight.lock().retain(|(held, _)| *held != id);
    }

    /// 备下一条成员的位置（可失败的准备；调用方在同一提交段里紧接着写入）。
    pub(crate) fn reserve_binding(&self) -> Result<(), PendFail> {
        let mut members = self.bindings.lock();
        members.try_reserve(1).map_err(|_| PendFail::OoM)
    }

    /// 写入一条成员：容量已在同一提交段备好，这一步只推入。
    pub(crate) fn register_binding(&self, binding: Arc<MappingBinding>) {
        self.bindings.lock().push(binding);
    }

    /// 撒下一条成员：这份绑定已结清（`Nest::retire_mapping` / `reap_mappings` 的唯一收尾）。
    ///
    /// 最后一条成员撤下就是显式的“结清”事件：来源不再占着任何东西时顺带向上收尾，
    /// 不在 `Drop` 里猜。
    pub(crate) fn release_binding(self: &Arc<Self>, binding: &Arc<MappingBinding>) {
        self.bindings
            .lock()
            .retain(|held| !Arc::ptr_eq(held, binding));
    }

    /// External access blocks private CPU publication until trusted settlement.
    pub(crate) fn ensure_internal(&self) -> Result<(), PendFail> {
        if self.external.load(Ordering::Acquire) {
            Err(PendFail::Busy)
        } else {
            Ok(())
        }
    }

    /// External responsibility and overlapping in-flight ownership changes block publication.
    pub(crate) fn check_publish(
        &self,
        offset: PageOffset,
        count: PageCount,
    ) -> Result<(), PendFail> {
        self.ensure_internal()?;
        let claimed = self.inflight.lock().iter().any(|(_, window)| {
            window
                .as_ref()
                .is_some_and(|window| window.overlaps(offset, count))
        });
        if claimed {
            return Err(PendFail::Busy);
        }
        Ok(())
    }

    /// 未结 CPU 身份的快照；不复制关系或物理持有。
    pub(crate) fn bindings(&self) -> Result<Vec<Arc<MappingBinding>>, PendFail> {
        let held = self.bindings.lock();
        let mut members = Vec::new();
        members.try_reserve(held.len()).map_err(|_| PendFail::OoM)?;
        members.extend(held.iter().cloned());
        Ok(members)
    }

    /// 已失活来源仍须保留补关入口的义务。
    pub(crate) fn pending_close(&self) -> bool {
        !self.bindings.lock().is_empty() || !self.inflight.lock().is_empty()
    }

    /// 这份来源登过关系的目标空间：由成员快照派生并去重。
    ///
    /// 先在源锁下克隆成员再放锁升级：最后一份 `Nest` 强引用不在源锁里析构。
    pub(crate) fn spaces(&self) -> Result<Vec<Arc<Nest>>, PendFail> {
        let members = self.bindings()?;
        let mut spaces = Vec::new();
        spaces
            .try_reserve(members.len())
            .map_err(|_| PendFail::OoM)?;
        for binding in &members {
            if let Some(space) = binding.nest().upgrade()
                && !spaces.iter().any(|held| Arc::ptr_eq(held, &space))
            {
                spaces.push(space);
            }
        }
        Ok(spaces)
    }

    /// 这份来源名下是否仍有这一份未结绑定。
    pub(crate) fn owns_binding(&self, binding: &Arc<MappingBinding>) -> bool {
        self.bindings
            .lock()
            .iter()
            .any(|held| Arc::ptr_eq(held, binding))
    }

    /// 撤销这份来源：在途发布未结清时返回 `Busy`，由调用方重试。
    /// 具名拿闸门，不在 match 侦式里再取同一把锁。
    pub(crate) fn revoke(&self) -> Result<(), PendFail> {
        let mut gate = self.revoked.lock();
        if !self.inflight.lock().is_empty() {
            return Err(PendFail::Busy);
        }
        *gate = true;
        Ok(())
    }

    /// 这份来源是否已被撤销：**短读**一次闸门，不把闸门留到本次操作结束。
    ///
    /// 授权已经交出去的引用（根 Pend）看的是同一条闸门，所以迁移不需要
    /// 另持一条锁跨整个执行：真正的串行由调用者的 pend 负责。
    pub(crate) fn revoked(&self) -> bool {
        *self.revoked.lock()
    }

    /// 这份来源当前的**管理者**是不是 `task`：它当场能不能替这份来源作主。
    ///
    /// 私有阶段只有真正的属主；交给根 Pend 之后，调用者表里那枚 **root token**
    /// （`sire` 为空的原始一枚）必须与相位记的是同一枚，pend_entry、Share 与根 Pend 都还活，
    /// 且借用状态允许当前访问；不以 `Arc` 强引用计数代替授权。
    ///
    /// 相位里的 Share 只按弱身份比对；活与同一枚都由表里那一枚 `pend_entry` 自己给出，
    /// 所以调用方须在同 Task 的 pend 之下调它：表不变，才有“临时强引用不会是
    /// 最后一份”这条持有链。
    pub(crate) fn manages(&self, task: &crate::basis::unit::task::Task) -> bool {
        if self.revoked() {
            return false;
        }
        // 短锁里克隆相位再判：放锁之后才取 pend_entry 表与 Pend 各自的锁，不与
        // “先 Pend 后相位”的读法（`Hold::resolve`）撞成逆序。
        let phase = Phase::clone(&self.phase.lock());
        match phase {
            Phase::Private => self.owner == task.ident.id,
            Phase::Root {
                meta,
                token,
                authority,
            } => {
                // 表里没有那一枚：调用者不持 root token，当场不是管理者。
                let Some(pend_entry) = super::locate(task, token) else {
                    return false;
                };
                pend_entry.sire().is_none()
                    && pend_entry.authority().modifiable().is_some()
                    // `pend_entry.alive()` 含这份 View（Share）自己活着；它与下面“同 Arc”
                    // 合起来就是“同一枚 Share 且活”，不只是 Pend 还在答话。
                    && pend_entry.alive()
                    && Arc::ptr_eq(pend_entry.authority(), &authority)
                    // 那一下克隆出的临时强引用不会是最后一份：表里这一枚 `pend_entry` 自己
                    // 就持着它，而这张表在调用者的 pend 之下不会变。
                    && pend_entry
                        .view()
                        .share_view()
                        .is_some_and(|held| Weak::as_ptr(&meta) == Arc::as_ptr(&held))
            }
        }
    }

    /// 已接手的根 Pend、它的 token 与**根 Pend 自己的**那枚 `Pend`。
    /// 还没接手就是 `None`。三段来自同一份相位，调用方不再各自去表里另找一遍。
    pub(crate) fn root(&self) -> Option<(Arc<ShareView>, PendId, Arc<Pend>)> {
        match &*self.phase.lock() {
            Phase::Root {
                meta,
                token,
                authority,
            } => Some((meta.upgrade()?, *token, authority.clone())),
            Phase::Private => None,
        }
    }

    /// 登记一份普通引用。
    pub(crate) fn claim_hold(&self) {
        self.holds.fetch_add(1, Ordering::AcqRel);
    }

    /// 解除一份普通引用。最后一份普通引用消失时，**未消费的一次性资格显式结束**。
    ///
    /// 这一定不靠 `Arc` 计数也不再靠授权记录被回收：接上 Mapping 登记后仍有人持着
    /// 这份记录，资格该结束还是结束。
    pub(crate) fn release_hold(&self) {
        if self.holds.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.qualification.consume();
        }
    }

    /// 一次性创建资格：单次 `claim` / `consume`，协议与既有 `Creation` 同一套。
    pub(crate) fn claim(&self) -> Result<(), PendFail> {
        self.qualification.claim()
    }

    pub(crate) fn release_claim(&self) {
        self.qualification.release();
    }

    /// 一次性资格用掉了（Share 已接手）。
    pub(crate) fn consume(&self) {
        self.qualification.consume();
    }

    /// 把私有根交给新根 Pend：真实范围同时从授权记录转出（从此由 Share 持有），
    /// 并返回私有阶段已建的关系，让创建事务把它们登到新 token 下。
    /// 已经接手过就拒（一个来源只有一条根）。
    pub(crate) fn adopt(
        &self,
        meta: &Arc<ShareView>,
        token: PendId,
        authority: Arc<Pend>,
    ) -> Result<(), PendFail> {
        let mut phase = self.phase.lock();
        if matches!(&*phase, Phase::Root { .. }) {
            return Err(PendFail::Denied);
        }
        // 真实范围先交给 Share，再改相位：交接期间授权记录不再强持来源。
        *self.range.lock() = None;
        *phase = Phase::Root {
            meta: Arc::downgrade(meta),
            token,
            authority,
        };
        Ok(())
    }

    /// 事务中途失败：把刚交出去的范围放回私有阶段。
    pub(crate) fn restore_private(&self, range: FlatRange) {
        let mut phase = self.phase.lock();
        *self.range.lock() = Some(range);
        *phase = Phase::Private;
    }
}

unsafe impl Send for SourceAuthorization {}
unsafe impl Sync for SourceAuthorization {}
