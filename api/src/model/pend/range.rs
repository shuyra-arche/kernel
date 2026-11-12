//! `Accord` 的授权范围：继承父窗口，或按父窗口起点的相对页偏移收窄。
//!
//! 这是授权坐标，不是 CPU 视图或物理引用：`Pages` 的 `offset` 相对父分支。
//! `Inherit` 不带任何载荷；`Pages` 非空且页坐标不溢出。

use crate::abi::register::{Decode, Wire};

use crate::model::cell::PAGE_SIZE;

/// 本地派生或跨 Task 授权请求的范围。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendRange {
    /// 继承父分支当前的有效窗口。
    Inherit,
    /// 请求父窗口起点之后 `offset` 页开始的 `count` 页。
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

    pub const fn words(self) -> [usize; Self::WORDS] {
        match self {
            Self::Inherit => [0, 0, 0],
            Self::Pages { offset, count } => [1, offset, count],
        }
    }

    pub fn of(words: [usize; Self::WORDS]) -> Option<Self> {
        match words {
            [0, 0, 0] => Some(Self::Inherit),
            [1, offset, count] => Self::pages(offset, count),
            _ => None,
        }
    }
}

impl Wire for PendRange {
    const WORDS: usize = PendRange::WORDS;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        for word in self.words() {
            word.pack(s, i);
        }
    }

    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let tag = usize::unpack(s, i)?;
        let offset = usize::unpack(s, i)?;
        let count = usize::unpack(s, i)?;
        Self::of([tag, offset, count]).ok_or(Decode::Invalid)
    }
}
