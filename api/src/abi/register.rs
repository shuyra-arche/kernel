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
