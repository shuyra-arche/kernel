use crate::abi::register::{Decode, Wire};

use core::ops::Add;
use core::ops::AddAssign;
use core::ops::Sub;
use core::ops::SubAssign;

use super::PAGE_SIZE;

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VirtAddr(pub(crate) usize);

impl VirtAddr {
    pub const fn new(addr: usize) -> Self {
        Self(addr)
    }
    pub const fn get(self) -> usize {
        self.0
    }

    #[inline]
    pub const fn wrap(addr: usize) -> Self {
        Self(addr)
    }

    #[inline]
    pub fn offset(self) -> usize {
        self.0 & (PAGE_SIZE - 1)
    }

    #[inline]
    pub fn page_align(self) -> Self {
        Self(self.0 & !(PAGE_SIZE - 1))
    }

    #[inline]
    pub const fn as_usize(self) -> usize {
        self.0
    }
}

impl Add<usize> for VirtAddr {
    type Output = Self;
    #[inline]
    fn add(self, rhs: usize) -> Self {
        Self(self.0 + rhs)
    }
}

impl Sub<usize> for VirtAddr {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: usize) -> Self {
        Self(self.0 - rhs)
    }
}

impl AddAssign<usize> for VirtAddr {
    fn add_assign(&mut self, rhs: usize) {
        self.0 += rhs;
    }
}

impl SubAssign<usize> for VirtAddr {
    fn sub_assign(&mut self, rhs: usize) {
        self.0 -= rhs;
    }
}

impl core::fmt::Debug for VirtAddr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "VA({:#x})", self.0)
    }
}

impl From<VirtAddr> for usize {
    fn from(value: VirtAddr) -> usize {
        value.0
    }
}

impl Wire for VirtAddr {
    const WORDS: usize = 1;
    fn pack(&self, s: &mut [usize; 6], i: &mut usize) {
        s[*i] = self.get();
        *i += 1;
    }
    fn unpack(s: &[usize; 6], i: &mut usize) -> Result<Self, Decode> {
        let v = *s.get(*i).ok_or(Decode::Overflow)?;
        *i += 1;
        Ok(VirtAddr(v))
    }
}
