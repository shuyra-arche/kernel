//! `Detach` / `SetAccess` 的选择：整段，或创建时定下的页坐标窗口。
//!
//! `Pages` 的坐标以**创建时的来源页窗口**为准，不因中间形成空洞而重排；
//! 选到已经关闭的页就是失败。两种选择调同一对入口。

use super::pages;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewSelection {
    Whole,
    Pages { offset: usize, count: usize },
}

impl ViewSelection {
    /// 过线的字数：标签 + 偏移 + 页数。
    pub const WORDS: usize = 3;

    pub fn pages(offset: usize, count: usize) -> Option<Self> {
        pages(offset, count)?;
        Some(Self::Pages { offset, count })
    }
}
