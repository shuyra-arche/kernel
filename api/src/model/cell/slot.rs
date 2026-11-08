//! 一份来源持有在**持有它的那张表**里的句柄（0 = 无效哨兵）。
//!
//! 与 [`PendId`](crate::model::identity::PendId) 同族：号只在自己那张表里解析得动，故本类型也
//! `!Send + !Sync`——跨线程只剩「交出裸值、对端另落一枚」这一条路。不装内核指针。

use crate::abi::layout::{WORD_BYTES, fetch_words, store_words};
use crate::abi::register::{Decode, Wire};
use crate::abi::reply::FromPair;

use core::marker::PhantomData;

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
pub struct SlotId(pub(crate) usize, PhantomData<*const ()>);

impl SlotId {
    /// 无效哨兵（0）：没有这一份持有。
    pub const NONE: Self = Self(0, PhantomData);

    /// 号在**线上**的宽度（8 字节 LE）——与 [`PendId`](crate::model::identity::PendId) 同形。
    pub const WIDTH: usize = 8;

    /// 由裸值造一枚（线上解码面）。
    pub(crate) const fn new(v: usize) -> Self {
        Self(v, PhantomData)
    }

    /// **收号**：运行时要记住一枚句柄再交回去（失败重试），走的就是这扇
    /// "收到一枚号"的门；伪造的号在自己那张表里查不到 ⇒ 照旧 Denied。
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let at = bytes.get(..Self::WIDTH)?;
        let raw = u64::from_le_bytes(at.try_into().ok()?);
        let raw = usize::try_from(raw).ok()?;
        (raw <= isize::MAX as usize).then(|| Self::new(raw))
    }

    /// **交出**：号变回线上的那 8 字节（与 [`SlotId::from_bytes`] 成对）。
    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        (self.0 as u64).to_le_bytes()
    }

    pub const fn get(self) -> usize {
        self.0
    }
}

impl From<SlotId> for usize {
    fn from(reference: SlotId) -> Self {
        reference.0
    }
}

impl Wire for SlotId {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        s[*i] = self.0;
        *i += 1;
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let v = *s.get(*i).ok_or(Decode::Overflow)?;
        *i += 1;
        if v > isize::MAX as usize {
            return Err(Decode::Invalid);
        }
        Ok(Self::new(v))
    }
}

impl FromPair for SlotId {
    fn from_pair(v0: usize, _v1: usize) -> Self {
        SlotId::new(v0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReferenceRange {
    pub reference: SlotId,
    pub offset: usize,
    pub count: usize,
}

impl ReferenceRange {
    pub fn new(reference: SlotId, offset: usize, count: usize) -> Option<Self> {
        super::pages(offset, count)?;
        Some(Self {
            reference,
            offset,
            count,
        })
    }

    /// 过线的字数；也是它在用户内存里作定长描述时的字数（同一份布局）。
    pub const WORDS: usize = 3;

    /// 把自身写进定长描述字；与 [`Wire`] 是同一份布局，不重写第二份。
    pub fn encode(self) -> [usize; Self::WORDS] {
        let mut buffer = [0usize; 6];
        let mut at = 0usize;
        self.pack(&mut buffer, &mut at);
        debug_assert_eq!(at, Self::WORDS);
        let mut words = [0usize; Self::WORDS];
        words.copy_from_slice(&buffer[..Self::WORDS]);
        words
    }

    /// 由定长描述字解码；同一份 checked 口径，非法位 → `Err(Decode)`。
    pub fn decode(words: [usize; Self::WORDS]) -> Result<Self, Decode> {
        let mut buffer = [0usize; 6];
        buffer[..Self::WORDS].copy_from_slice(&words);
        let mut at = 0usize;
        let value = Self::unpack(&buffer, &mut at)?;
        if at != Self::WORDS {
            return Err(Decode::Invalid);
        }
        Ok(value)
    }

    pub const WIDTH: usize = Self::WORDS * WORD_BYTES;

    /// Write the fixed ABI descriptor. `out` must have exactly `WIDTH` bytes.
    pub fn store(&self, out: &mut [u8]) {
        store_words(self.encode(), out);
    }

    pub fn fetch(bytes: &[u8]) -> Option<Self> {
        Self::decode(fetch_words(bytes)?).ok()
    }
}

impl Wire for ReferenceRange {
    const WORDS: usize = ReferenceRange::WORDS;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        self.reference.pack(s, i);
        self.offset.pack(s, i);
        self.count.pack(s, i);
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let reference = SlotId::unpack(s, i)?;
        let offset = usize::unpack(s, i)?;
        let count = usize::unpack(s, i)?;
        Self::new(reference, offset, count).ok_or(Decode::Invalid)
    }
}
