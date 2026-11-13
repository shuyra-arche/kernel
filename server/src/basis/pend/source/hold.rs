//! A Task-local owning Slot or internal authority-window handle. It carries no
//! independent Retain privilege. After a Share adopts its source, every resolution
//! revalidates the exact Pend branch and window; hardware residency is separate.

use crate::metal::sync::SpinLock;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use kernel_api::abi::call::pend::PendFail;

use crate::basis::flat::FlatRange;
use crate::basis::nest::Access;
use crate::basis::pend::Pend;
use kernel_api::address::PageCount;
use kernel_api::address::PageOffset;
use kernel_api::model::identity::PendId;

use super::SourceAuthorization;

pub(crate) struct Hold {
    id: usize,
    source: Arc<SourceAuthorization>,
    offset: PageOffset,
    count: PageCount,
    ceiling: Access,
    /// 这份引用是从**哪条 Pend 分支**取得的（Share 的哪一枚 PendEntry）。
    /// 撤销边与上限都按这一条算，不因同 Flat、共享根资格就并到根上去。
    branch: Option<(PendId, Arc<Pend>)>,
}

impl Hold {
    pub(super) fn new(
        id: usize,
        source: Arc<SourceAuthorization>,
        offset: PageOffset,
        count: PageCount,
        ceiling: Access,
        branch: Option<(PendId, Arc<Pend>)>,
    ) -> Self {
        Self {
            id,
            source,
            offset,
            count,
            ceiling,
            branch,
        }
    }

    pub(crate) fn id(&self) -> usize {
        self.id
    }

    pub(crate) fn source(&self) -> &Arc<SourceAuthorization> {
        &self.source
    }

    /// 相对来源授权整段的页窗口。
    pub(crate) fn window(&self) -> (PageOffset, PageCount) {
        (self.offset, self.count)
    }

    pub(crate) fn ceiling(&self) -> Access {
        self.ceiling
    }

    /// 这条分支（Share 的 token 与它的 Pend）；私有来源没有。
    pub(crate) fn branch(&self) -> Option<(PendId, &Arc<Pend>)> {
        self.branch
            .as_ref()
            .map(|(token, authority)| (*token, authority))
    }

    /// 这条分支的 Pend（没有分支就是 `None`）。
    pub(crate) fn authority(&self) -> Option<&Arc<Pend>> {
        self.branch.as_ref().map(|(_, authority)| authority)
    }

    /// Resolve current source and branch authority without creating a hardware hold.
    pub(crate) fn resolve(&self) -> Option<FlatRange> {
        self.resolve_window(self.offset, self.count)
    }

    /// Absolute source coordinates stay within this Hold's immutable bounds.
    pub(crate) fn resolve_window(&self, offset: PageOffset, count: PageCount) -> Option<FlatRange> {
        let end = offset.get().checked_add(count.get())?;
        let limit = self.offset.get().checked_add(self.count.get())?;
        if offset.get() < self.offset.get()
            || end > limit
            || !self
                .source
                .branch_check(self.authority().map(Arc::as_ref), Access::READ)
        {
            return None;
        }
        if let Some((_, authority)) = &self.branch {
            use kernel_api::model::pend::{
                FlatRights, Operations, PendRange, PendScope, ShareRights,
            };
            let scope = PendScope::Pages(PendRange::Pages {
                offset: offset.get(),
                count: count.get(),
            });
            if !authority.selects(Operations::Flat(FlatRights::ASSOCIATE), scope)
                && !authority.selects(Operations::Share(ShareRights::ASSOCIATE), scope)
            {
                return None;
            }
            let (meta, _, _) = self.source.root()?;
            if !meta.alive() {
                return None;
            }
            return meta.source().range(offset, count).ok();
        }
        self.source.window(offset, count)
    }
}

unsafe impl Send for Hold {}
unsafe impl Sync for Hold {}

// 一个 Task 的**引用表**：它登记的那些普通引用。
//
// 句柄号是**全局单调**的（铸一次、永不复用），所以旧号不可能在后来的表里命中另一个对象，
// 跨 Task 的同一个数字也不成立——表自己按 id 查找，找不到就是不属于这张表。
// 号到符号上界就到了：那是运行时守住的条件，不用 debug 断言代替。

pub(crate) struct HoldTable {
    holds: SpinLock<Vec<Arc<Hold>>>,
    sources: SpinLock<Vec<Weak<SourceAuthorization>>>,
}

/// 铸一个全局唯一的引用号；耗尽（或超出符号上界）即明确失败，不回收旧号。
fn next_id() -> Result<usize, PendFail> {
    static NEXT: crate::basis::name::Serial =
        crate::basis::name::Serial::new(1, isize::MAX as usize);
    NEXT.allocate().ok_or(PendFail::OutOfMemory)
}

impl HoldTable {
    pub(crate) const fn new() -> Self {
        Self {
            holds: SpinLock::new(Vec::new()),
            sources: SpinLock::new(Vec::new()),
        }
    }

    /// 登记一份新引用；容量与弧先备齐，失败不留下半份引用。
    pub(crate) fn insert(
        &self,
        source: Arc<SourceAuthorization>,
        offset: PageOffset,
        count: PageCount,
        ceiling: Access,
        branch: Option<(
            kernel_api::model::identity::PendId,
            Arc<crate::basis::pend::Pend>,
        )>,
    ) -> Result<usize, PendFail> {
        let mut holds = self.holds.lock();
        if holds.len() >= 1024 {
            return Err(PendFail::OutOfMemory);
        }
        holds.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
        let mut sources = self.sources.lock();
        sources.retain(|source| source.strong_count() != 0);
        let known = sources
            .iter()
            .any(|known| known.as_ptr() == Arc::as_ptr(&source));
        if !known {
            sources.try_reserve(1).map_err(|_| PendFail::OutOfMemory)?;
        }
        let id = next_id()?;
        let hold = Arc::try_new(Hold::new(id, source, offset, count, ceiling, branch))
            .map_err(|_| PendFail::OutOfMemory)?;
        if !known {
            sources.push(Arc::downgrade(hold.source()));
        }
        hold.source().claim_hold();
        holds.push(hold);
        Ok(id)
    }

    pub(crate) fn get(&self, id: usize) -> Option<Arc<Hold>> {
        self.holds
            .lock()
            .iter()
            .find(|hold| hold.id() == id)
            .cloned()
    }

    /// 解除这一份引用；只从这个表里摘掉它，不动其他引用，也不分配。
    pub(crate) fn remove(&self, id: usize) -> Option<Arc<Hold>> {
        let mut holds = self.holds.lock();
        let at = holds.iter().position(|hold| hold.id() == id)?;
        let hold = holds.remove(at);
        drop(holds);
        hold.source().release_hold();
        Some(hold)
    }

    /// A removed Slot can still source foreign publications. Keep discovery until
    /// this actor's exact uses settle, before discarding its last ledger/husk.
    pub(crate) fn retire(
        &self,
        task: &Arc<crate::basis::unit::task::Task>,
    ) -> Result<bool, PendFail> {
        let mut retired = 0;
        for _ in 0..4 {
            let source = {
                let mut sources = self.sources.lock();
                sources.retain(|source| source.strong_count() != 0);
                sources.first().and_then(Weak::upgrade)
            };
            let Some(source) = source else {
                break;
            };
            for binding in source.bindings().map_err(|_| PendFail::Busy)? {
                let nest = binding.nest().upgrade().ok_or(PendFail::Busy)?;
                if nest
                    .binding_source(&binding)
                    .is_some_and(|selected| selected.belongs_to(task.ident.id))
                {
                    nest.withdraw_source(&binding, &source, None, Some(task.ident.id), true)?;
                    retired += 1;
                    if retired == 4 {
                        return Ok(false);
                    }
                }
            }
            self.sources
                .lock()
                .retain(|known| known.as_ptr() != Arc::as_ptr(&source));
        }
        if !self.sources.lock().is_empty() {
            return Ok(false);
        }
        for _ in 0..4 {
            let Some(hold) = self.holds.lock().pop() else {
                return Ok(true);
            };
            hold.source().release_hold();
        }
        Ok(self.holds.lock().is_empty())
    }
}

unsafe impl Send for HoldTable {}
unsafe impl Sync for HoldTable {}
