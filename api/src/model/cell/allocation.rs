//! 新 RAM 来源的物理分配约束（过线的那一格）。

use crate::abi::register::{Decode, Wire};

use crate::address::BASE_PAGE_SIZE;

/// 新 RAM 来源：`size` 至少一个 4 KiB ABI 基础单位，合法长度向上取整；
/// `align` 是每个独立物理 Page 起址的对齐下界。
///
/// 此请求可以由多个互不连续的 4 KiB Page 满足。即使 `size` 是 2 MiB 或
/// 1 GiB，它也不表示单个连续物理 Cell，或授予某种 MMU 巨页承诺。
/// 对齐只约束**每页起址**，不代表各页物理连续；不支持的约束在构造时拒绝。
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
        if self.size < BASE_PAGE_SIZE || !self.align.is_power_of_two() {
            return None;
        }
        self.size.checked_next_multiple_of(BASE_PAGE_SIZE)
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
