//! `Derive` 的授权范围：继承父窗口，或按来源起点的绝对页偏移收窄。
//!
//! 这是授权坐标，不是 CPU 视图或物理引用：`Pages` 的 `offset` 相对底层来源起点，
//! 不相对父分支。`Inherit` 不带任何载荷；`Pages` 非空且页坐标不溢出。

use crate::model::memory::PAGE_SIZE;

/// 本地派生请求的范围。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendRange {
    /// 继承父分支当前的有效窗口。
    Inherit,
    /// 请求来源起点之后 `offset` 页开始的 `count` 页。
    Pages { offset: usize, count: usize },
}

impl PendRange {
    /// 过线的字数：标签 + 偏移 + 页数。
    pub const WORDS: usize = 3;

    /// 非空且页坐标、字节跨度都不溢出的窗口。
    pub fn pages(offset: usize, count: usize) -> Option<Self> {
        if count == 0 {
            return None;
        }
        let end = offset.checked_add(count)?;
        end.checked_mul(PAGE_SIZE)?;
        Some(Self::Pages { offset, count })
    }
}
