use alloc::sync::Arc;

use crate::basis::flat::FlatRange;
use crate::basis::nest::NestError;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PageCount;
use kernel_api::address::PageOffset;

use super::span::Span;

/// 一段关系上的选择。
///
/// `Whole` 只是一个标记：操作时按“当前活动 ∪ 未结算退休”解析，所以部分撤下之后
/// 剩下的就是还欠着的那几段。`Pages` 是创建时定下的页坐标：它必须整段都还在本
/// 关系的名下（活动或未结算退休），落在已经结算完的洞上就是失败 —— 选择不做
/// 静默过滤，也就不存在“选到已关闭页却当没事”。
#[derive(Clone)]
pub(crate) enum NestRange {
    Whole(Arc<Span>),
    Pages {
        mapping: Arc<Span>,
        offset: PageOffset,
        count: PageCount,
    },
}

impl NestRange {
    /// 初始范围内的固定页坐标选择；越界或零页即拒。
    pub(super) fn pages(
        mapping: &Arc<Span>,
        offset: PageOffset,
        count: PageCount,
    ) -> Result<Self, NestError> {
        let end = offset
            .get()
            .checked_add(count.get())
            .ok_or(NestError::NotAligned)?;
        if end
            .checked_mul(PAGE_SIZE)
            .is_none_or(|end| end > mapping.initial_size())
        {
            return Err(NestError::NoRegion);
        }
        Ok(Self::Pages {
            mapping: mapping.clone(),
            offset,
            count,
        })
    }

    pub(crate) fn mapping(&self) -> &Arc<Span> {
        match self {
            Self::Whole(mapping) => mapping,
            Self::Pages { mapping, .. } => mapping,
        }
    }

    /// 选择的字节窗口（含尾）。`Whole` 取整个初始范围；含尾表示因此顶页也不溢出。
    pub(crate) fn window(&self) -> (usize, usize) {
        match self {
            Self::Whole(mapping) => {
                let start = mapping.initial_start().as_usize();
                (start, start + (mapping.initial_size() - 1))
            }
            Self::Pages {
                mapping,
                offset,
                count,
            } => {
                let start = mapping.initial_start().as_usize() + offset.get() * PAGE_SIZE;
                (start, start + (count.get() * PAGE_SIZE - 1))
            }
        }
    }

    /// 源在本选择内的子范围：强持同一来源、不复制数据。
    pub(crate) fn source(&self) -> Result<FlatRange, NestError> {
        let mapping = self.mapping();
        let (offset, count) = match self {
            Self::Whole(_) => (
                PageOffset::ZERO,
                PageCount::new(mapping.initial_size() / PAGE_SIZE).ok_or(NestError::NotAligned)?,
            ),
            Self::Pages { offset, count, .. } => (*offset, *count),
        };
        Ok(mapping.source().range(offset, count)?)
    }
}
