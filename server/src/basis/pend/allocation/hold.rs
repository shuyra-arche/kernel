//! 一份**普通引用**：某份来源授权下的一段页窗口与它的访问上限。
//!
//! 引用只登记在**持有它的那个 Task** 的引用表里，所以句柄是表的作用域；
//! 它不搬页、不占 VA、也不持有 `Mapping`（`Release` 因此不隐式 `Detach`）。
//!
//! **它不永久强持来源**：除了带独立保留的那一份，它只记住相对来源授权的页坐标，
//! 真正能用的范围每次向授权记录解析——来源被根 Pend 接管后，撤销即解析不到，
//! 普通引用不会在撤销后继续 pin 住帧。只有独立保留那一份自己强持那段范围，
//! 于是它才能跨 Driver 撤销继续持同一批帧。

use alloc::sync::Arc;

use crate::basis::flat::FlatRange;
use crate::basis::nest::Access;
use crate::basis::pend::authority::Pend;
use kernel_api::address::PageCount;
use kernel_api::address::PageOffset;
use kernel_api::model::identity::PendId;

use super::geometry::Geometry;
use super::independent::IndependentHold;
use super::source_authorization::SourceAuthorization;

pub(crate) struct Hold {
    id: usize,
    source: Arc<SourceAuthorization>,
    offset: PageOffset,
    count: PageCount,
    ceiling: Access,
    /// 独立持有：物理保留与它在来源记录上的登记一起生、一起灭。
    retained: Option<IndependentHold>,
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
        retained: Option<IndependentHold>,
        branch: Option<(PendId, Arc<Pend>)>,
    ) -> Self {
        Self {
            id,
            source,
            offset,
            count,
            ceiling,
            retained,
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

    pub(crate) fn retains(&self) -> bool {
        self.retained.is_some()
    }

    /// 这条引用自己那一份独立持有的窗口；没有独立持有就是 `None`。
    ///
    /// 使用判据按**这一份登记的身份**豁免：只有从它自己这条引用上取的用不算冲突。
    pub(crate) fn retention(&self) -> Option<&Arc<Geometry>> {
        self.retained.as_ref().map(IndependentHold::window)
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

    /// 这一段引用**当前**能拿到的来源范围。
    ///
    /// 独立持有自己强持；其余向来源授权解析：根 Pend 失效就返回 `None`，
    /// 于是未经保留的引用在撤销后既不能挂载也不再 pin 住来源。
    pub(crate) fn resolve(&self) -> Option<FlatRange> {
        if let Some(retained) = &self.retained {
            return Some(retained.range().clone());
        }
        if let Some((_, authority)) = &self.branch {
            authority.modifiable()?;
            let (meta, _, _) = self.source.root()?;
            if !meta.alive() {
                return None;
            }
            return meta.source().range(self.offset, self.count).ok();
        }
        self.source.window(self.offset, self.count)
    }
}

unsafe impl Send for Hold {}
unsafe impl Sync for Hold {}
