//! Logical page coordinates and lengths.

use core::num::NonZeroUsize;

use super::PAGE_SIZE;

/// 页数。非零：空范围没有单独表示，不能与"未声明"混为一谈。
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct PageCount(NonZeroUsize);

impl PageCount {
    /// 零不是合法的页数。
    #[inline]
    pub fn new(value: usize) -> Option<Self> {
        NonZeroUsize::new(value).map(Self)
    }

    #[inline]
    pub fn get(self) -> usize {
        self.0.get()
    }

    /// 页数折算字节；溢出返回 `None`。
    #[inline]
    pub fn checked_bytes(self) -> Option<usize> {
        self.0.get().checked_mul(PAGE_SIZE)
    }
}

/// 来源内的逻辑页号。它与物理地址、页内偏移是不同量纲，不互换、不参与地址运算。
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct PageIndex(usize);

impl PageIndex {
    pub const ZERO: Self = Self(0);

    #[inline]
    pub fn new(value: usize) -> Self {
        Self(value)
    }

    #[inline]
    pub fn get(self) -> usize {
        self.0
    }
}

/// 来源范围内的页偏移。相对于某个范围起点，不能与逻辑页号互换。
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct PageOffset(usize);

impl PageOffset {
    /// 窗口的第一页；健康用例按它取物化结果。

    pub const ZERO: Self = Self(0);

    #[inline]
    pub fn new(value: usize) -> Self {
        Self(value)
    }

    #[inline]
    pub fn get(self) -> usize {
        self.0
    }
}
