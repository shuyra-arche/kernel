//! Checked six-word request encodings.

pub trait Wire: Sized {
    /// Exact register count consumed by both pack and unpack.
    const WORDS: usize;
    /// 把自身 pack 进 `s`，游标 `i` 前进 `WORDS` 格。
    fn pack(&self, s: &mut [usize; 6], i: &mut usize);
    /// 从 `s` 读出自身，游标 `i` 前进 `WORDS` 格；非法位 → `Err(Decode)`。
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode>;
}

/// 解码错误：unpack 失败（含非法位校验拒绝）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decode {
    /// 未知调用号（index 越界）。
    BadSlot,
    /// 字段数超出 a0..a5。
    Overflow,
    /// 非法位（如 `Modifiable` 含未定义位，或 bool 非 0/1）。
    Invalid,
}

// ── 基元 ────────────────────────────────────────────────────────────────

impl Wire for usize {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        s[*i] = *self;
        *i += 1;
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let v = *s.get(*i).ok_or(Decode::Overflow)?;
        *i += 1;
        Ok(v)
    }
}

impl Wire for u32 {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        (*self as usize).pack(s, i);
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        u32::try_from(usize::unpack(s, i)?).map_err(|_| Decode::Invalid)
    }
}

impl Wire for u64 {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        s[*i] = *self as usize;
        *i += 1;
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let v = *s.get(*i).ok_or(Decode::Overflow)? as u64;
        *i += 1;
        Ok(v)
    }
}

impl Wire for crate::model::wait::Wait {
    const WORDS: usize = 2;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        let (tag, at): (usize, u64) = match self {
            Self::Poll => (0, 0),
            Self::At(at) => (1, *at),
            Self::Infinite => (2, 0),
        };
        tag.pack(s, i);
        (at as usize).pack(s, i);
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let tag = usize::unpack(s, i)?;
        let at = usize::unpack(s, i)?;
        match (tag, at) {
            (0, 0) => Ok(Self::Poll),
            (1, at) => Ok(Self::At(at as u64)),
            (2, 0) => Ok(Self::Infinite),
            _ => Err(Decode::Invalid),
        }
    }
}

impl Wire for bool {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        s[*i] = *self as usize;
        *i += 1;
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let v = *s.get(*i).ok_or(Decode::Overflow)?;
        *i += 1;
        match v {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Decode::Invalid),
        }
    }
}

impl Wire for crate::model::unit::ProgramKind {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        s[*i] = match self {
            crate::model::unit::ProgramKind::User => 0,
            crate::model::unit::ProgramKind::Supervisor => 1,
        };
        *i += 1;
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let v = *s.get(*i).ok_or(Decode::Overflow)?;
        *i += 1;
        match v {
            0 => Ok(crate::model::unit::ProgramKind::User),
            1 => Ok(crate::model::unit::ProgramKind::Supervisor),
            _ => Err(Decode::Invalid),
        }
    }
}

/// Decode Modifiable strictly: only whole-register values 0 and 1 are valid.
impl Wire for crate::model::modifiable::Modifiable {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        s[*i] = self.bits() as usize;
        *i += 1;
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let v = *s.get(*i).ok_or(Decode::Overflow)?;
        *i += 1;
        let bits = u32::try_from(v).map_err(|_| Decode::Invalid)?;
        crate::model::modifiable::Modifiable::from_bits(bits).ok_or(Decode::Invalid)
    }
}

mod cell;
mod clone_view;
mod identity;
mod pend;
mod sign;
mod unit;
