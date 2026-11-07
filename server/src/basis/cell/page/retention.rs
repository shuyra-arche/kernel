use alloc::sync::Arc;
use core::ops::Deref;

use super::Page;
use super::RetainError;
use super::reclaim::Kind;

#[derive(Debug, PartialEq, Eq)]
enum Phase {
    Prepared,
    Published,
    Retiring,
    Settled,
}

/// 内核持有的硬件访问责任和读写互斥预留。
/// 它落实已获授权的访问，不创建授权；不能复制为两个完成凭证。
#[derive(Debug)]
pub(crate) struct Retention {
    cell: Option<Arc<Page>>,
    phase: Phase,
    kind: Kind,
    modifiable: bool,
}

impl Retention {
    pub(super) fn prepare(
        cell: &Arc<Page>,
        kind: Kind,
        modifiable: bool,
    ) -> Result<Self, RetainError> {
        cell.reclaim.register(kind, modifiable)?;
        Ok(Self {
            cell: Some(cell.clone()),
            phase: Phase::Prepared,
            kind,
            modifiable,
        })
    }

    pub(super) fn during(
        change: &super::Change,
        kind: Kind,
        modifiable: bool,
    ) -> Result<Self, RetainError> {
        change.page().reclaim.register_during(kind, modifiable)?;
        Ok(Self {
            cell: Some(change.page().clone()),
            phase: Phase::Prepared,
            kind,
            modifiable,
        })
    }

    pub(crate) fn cell(&self) -> &Arc<Page> {
        self.cell.as_ref().expect("live retention")
    }

    /// 必须先于首条硬件可见发布。此后 Drop 不再视为安全取消。
    pub(crate) fn publish(&mut self) {
        assert_eq!(self.phase, Phase::Prepared);
        self.phase = Phase::Published;
    }

    /// 开始撤销；即使提交或同步失败，仍保留责任直至可信结清。
    pub(crate) fn retire(mut self) -> Self {
        assert_eq!(self.phase, Phase::Published);
        self.phase = Phase::Retiring;
        self
    }

    /// # Safety
    /// 调用方必须证明本凭证对应的访问已撤销，相关翻译及在途访问均已结清。
    /// Unit 退出、通知、请求已提交或用户态 Ack 都不构成这个证明。
    pub(crate) unsafe fn settle(mut self) {
        assert_eq!(self.phase, Phase::Retiring);
        self.cell().reclaim.discharge(self.kind, self.modifiable);
        self.phase = Phase::Settled;
    }
}

impl Deref for Retention {
    type Target = Page;
    fn deref(&self) -> &Page {
        self.cell()
    }
}

impl Drop for Retention {
    fn drop(&mut self) {
        match self.phase {
            Phase::Prepared => self.cell().reclaim.discharge(self.kind, self.modifiable),
            Phase::Settled => {}
            Phase::Published | Phase::Retiring => {
                // 非正常遗失责任时永久隔离：保留身份、账目和物理分配，禁止新登记。
                // 这是无分配的 fail-closed 兜底，不是可枚举/可恢复的隔离队列。
                self.cell().reclaim.abandon();
                core::mem::forget(self.cell.take());
            }
        }
    }
}
