//! A Task-local owning Slot or internal authority-window handle. It carries no
//! independent Retain privilege. After a Share adopts its source, every resolution
//! revalidates the exact Pend branch and window; hardware residency is separate.

use alloc::sync::Arc;

use crate::basis::flat::FlatRange;
use crate::basis::nest::Access;
use crate::basis::pend::authority::Pend;
use kernel_api::address::PageCount;
use kernel_api::address::PageOffset;
use kernel_api::model::identity::PendId;

use super::source_authorization::SourceAuthorization;

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
        if let Some((_, authority)) = &self.branch {
            authority.modifiable()?;
            if !authority.contains(self.offset, self.count) {
                return None;
            }
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
