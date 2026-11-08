//! Atomic Slot movement. Page identity and existing Memo obligations survive.
//! Withdrawal is an explicit Memo Pull, never an implicit ownership effect.
mod error;

pub use error::TransferError;

use crate::basis::nest::Mapping;
use alloc::sync::Weak;
use alloc::vec::Vec;

use crate::basis::flat::FlatRange;
use crate::basis::flat::transaction::Transaction;
use crate::basis::nest::source;
use kernel_api::address::PageIndex;
use kernel_api::address::PageOffset;

// 一次移交的结果载荷：迁移结果与它动过的活关系，两条通道交给上层。
//
// 清单只表达 core 关系事实——两端各自压着这一页的那些关系——不认识 Room、
// Task 或 Work；怎么挤醒等待者由上层决定。`must_use` 只提醒把这份清单消费掉，
// 不强制通知顺序，也不阻止调用方只取 `result` 而丢弃 `affected`。

/// 一次移交的完整结果。
#[must_use = "the affected relations must reach whoever wakes their waiters"]
pub struct TransferOutcome {
    /// 本次移交的成败。占槽前的早错也为 `Err`，那时 `affected` 为空。
    pub result: Result<(), TransferError>,
    /// 本次移交动过的活关系：压着源页或目标页的那些。占槽前失败时为空。
    pub affected: Vec<Weak<Mapping>>,
}

/// 一次移交：两端各自页号相对各自窗口，不是 Flat 绝对页号。
pub struct Transfer {
    source: FlatRange,
    source_page: PageIndex,
    target: FlatRange,
    target_page: PageIndex,
}

impl Transfer {
    /// 换两端绝对页号：两端各自显式检查范围；同一份来源上的同一页当场拒。
    pub fn new(
        source: FlatRange,
        source_page: PageOffset,
        target: FlatRange,
        target_page: PageOffset,
    ) -> Result<Self, TransferError> {
        let source_page = source
            .page_at(source_page)
            .map_err(|_| TransferError::OutOfRange)?;
        let target_page = target
            .page_at(target_page)
            .map_err(|_| TransferError::OutOfRange)?;
        Ok(Self {
            source,
            source_page,
            target,
            target_page,
        })
    }

    /// 收下两端候选，原子移动唯一 Slot 持有；已有 Memo 的 Page 责任保持有效。
    ///
    /// 两端候选在占槽的同一个临界区里收全（只收 `Weak`），所以一占上 `Moving`，
    /// 就没有新的关系能再压到这一页上。占槽后的失败会把两端先还原，再把结果连同
    /// 同一份关系清单交回；中途不用 `?` 抖掉清单。
    pub fn execute(
        &self,
        change: Option<&alloc::sync::Arc<crate::basis::cell::page::Change>>,
    ) -> TransferOutcome {
        let claimed = Transaction::claim(
            &self.source,
            self.source_page,
            &self.target,
            self.target_page,
            change,
            || {
                source::candidates_over(&self.source, &self.target)
                    .map_err(|_| TransferError::OutOfMemory)
            },
        );
        let (transaction, candidates) = match claimed {
            Ok(pair) => pair,
            // 占槽前的早错：没有一格槽被改动，也没有关系被这次移交动过。
            Err(error) => {
                return TransferOutcome {
                    result: Err(error),
                    affected: Vec::new(),
                };
            }
        };
        // 占槽之后、不持任何 core 锁：候选是完整上界，这里按关系自己的窗口收回
        // 真正动过的那几条。
        let affected = source::affected_over(
            candidates,
            &self.source,
            self.source_page,
            &self.target,
            self.target_page,
        );
        transaction.commit();
        let result = Ok(());
        TransferOutcome { result, affected }
    }
}
