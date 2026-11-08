//! 新 RAM 来源的物理分配约束（过线的那一格）。

use crate::abi::register::{Decode, Wire};

use super::PAGE_SIZE;

/// 新 RAM 来源：`size` 至少 4 KiB，合法长度向上取整到页；`align` 是每页起址的对齐下界。
///
/// 对齐只约束**每页起址**的下界，不表示各页物理连续；不支持的约束在构造时明确拒绝，
/// 不接受后忽略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allocation {
    pub size: usize,
    pub align: usize,
}

impl Allocation {
    /// 过线的字数（[`Wire`](crate::abi::register::Wire) 一进一出都走这两格）。
    pub const WORDS: usize = 2;

    /// 小对象由 Unit 内分配器处理，不将不足一页的请求静默升级。
    pub fn new(size: usize, align: usize) -> Option<Self> {
        let allocation = Self { size, align };
        allocation.bytes()?;
        Some(allocation)
    }

    /// 重新校验公开字段并给出分配长度。可由多个 Page 满足，不承诺物理连续。
    pub fn bytes(self) -> Option<usize> {
        if self.size < PAGE_SIZE || !self.align.is_power_of_two() {
            return None;
        }
        self.size.checked_next_multiple_of(PAGE_SIZE)
    }
}

impl Wire for Allocation {
    const WORDS: usize = Allocation::WORDS;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        self.size.pack(s, i);
        self.align.pack(s, i);
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let size = usize::unpack(s, i)?;
        let align = usize::unpack(s, i)?;
        Self::new(size, align).ok_or(Decode::Invalid)
    }
}
