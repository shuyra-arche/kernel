use super::{Access, Attributes, Nest, NestError};
use crate::metal::generic::mmu::PageTable;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::VirtAddr;

impl Nest {
    /// 一段字节窗口是否整段落在用户可达的半边之内。
    pub(crate) fn user_range(at: VirtAddr, size: usize) -> bool {
        let ceiling = crate::metal::mmu::upper().as_usize();
        at.as_usize() < ceiling
            && at
                .as_usize()
                .checked_add(size)
                .is_some_and(|end| end <= ceiling)
    }

    /// 用户半边的一次访问判定。
    pub(crate) fn validate_access(&self, at: VirtAddr, len: usize, need: Access) -> bool {
        Self::user_range(at, len) && self.check_access(at, len, need)
    }

    pub(crate) fn copy_in(&self, dst: &mut [u8], at: VirtAddr) -> bool {
        Self::user_range(at, dst.len()) && self.copy_from(at, dst, Access::READ)
    }

    pub(crate) fn copy_out(&self, src: &[u8], at: VirtAddr) -> bool {
        Self::user_range(at, src.len()) && self.copy_to(at, src, Access::WRITE)
    }

    /// 取一段指令的首字节：取不到就没有可取的长度。
    pub(crate) fn instruction_byte(&self, at: VirtAddr) -> Option<u8> {
        let mut byte = [0];
        (Self::user_range(at, byte.len()) && self.copy_from(at, &mut byte, Access::EXECUTE))
            .then_some(byte[0])
    }

    fn check_access(&self, start: VirtAddr, size: usize, need: Access) -> bool {
        self.access_range(start, size, need, None)
    }

    /// 提交边界内的纯原子读；不能在这里推进 shootdown 或物化。
    pub(crate) fn load_u32(&self, address: VirtAddr) -> Result<u32, NestError> {
        use core::sync::atomic::{AtomicU32, Ordering};
        let at = address.as_usize();
        if at % core::mem::align_of::<AtomicU32>() != 0 {
            return Err(NestError::NotAligned);
        }
        let state = self.state.lock();
        if !state.retirement.invalidation().is_clean() {
            return Err(NestError::Busy);
        }
        if state.phase.is_closed() {
            return Err(NestError::NoRegion);
        }
        let mapping = state.index.find(at).ok_or(NestError::NoRegion)?;
        let inner = mapping.state.lock();
        let range = inner.range_at(at).ok_or(NestError::NoRegion)?;
        let page = address.page_align();
        let (physical, flags) = state.root.walk_ref_geometry(page, self.geometry)?;
        if !range.current.contains(Access::READ)
            || !flags.contains(Attributes::READ)
            || flags.contains(Attributes::GLOBAL)
            || (!self.kind.is_supervisor() && !flags.contains(Attributes::USER))
        {
            return Err(NestError::NoRegion);
        }
        // Nest and Mapping guards keep this resident cell mapped during the read.
        let ptr = (physical.as_usize() + (at & (PAGE_SIZE - 1))) as *mut u32;
        Ok(unsafe { AtomicU32::from_ptr(ptr) }.load(Ordering::Relaxed))
    }

    fn copy_from(&self, start: VirtAddr, dst: &mut [u8], need: Access) -> bool {
        self.access_range(start, dst.len(), need, Some((dst.as_mut_ptr(), false)))
    }

    fn copy_to(&self, start: VirtAddr, src: &[u8], need: Access) -> bool {
        self.access_range(
            start,
            src.len(),
            need,
            Some((src.as_ptr() as *mut u8, true)),
        )
    }

    fn access_range(
        &self,
        start: VirtAddr,
        size: usize,
        need: Access,
        buffer: Option<(*mut u8, bool)>,
    ) -> bool {
        if start.as_usize().checked_add(size).is_none() {
            return false;
        }
        let Ok(state) = self.ready_state() else {
            return false;
        };
        let user = !self.kind.is_supervisor();
        let mut offset = 0;
        while offset < size {
            let at = start.as_usize() + offset;
            let Some(mapping) = state.index.find(at) else {
                return false;
            };
            let inner = mapping.state.lock();
            let Some(range) = inner.range_at(at) else {
                return false;
            };
            let page = crate::metal::mmu::canonical(at & !(PAGE_SIZE - 1));
            let Ok((_, hardware)) = state.root.walk_ref_geometry(page, self.geometry) else {
                return false;
            };
            if !range.current.contains(need)
                || !hardware.contains(need.to_attributes())
                || hardware.contains(Attributes::GLOBAL)
            {
                return false;
            }
            if user && !hardware.contains(Attributes::USER) {
                return false;
            }
            let count = (PAGE_SIZE - (at & (PAGE_SIZE - 1))).min(size - offset);
            offset += count;
        }
        let Some((buffer, write_to_user)) = buffer else {
            return true;
        };
        offset = 0;
        while offset < size {
            let at = start.as_usize() + offset;
            let Some(mapping) = state.index.find(at) else {
                unreachable!("Nest held across access validation and copy")
            };
            let _inner = mapping.state.lock();
            let page = crate::metal::mmu::canonical(at & !(PAGE_SIZE - 1));
            let (physical, _) = state
                .root
                .walk_ref_geometry(page, self.geometry)
                .expect("validated page table leaf");
            let within = at & (PAGE_SIZE - 1);
            let count = (PAGE_SIZE - within).min(size - offset);
            let physical = physical.as_usize() as *mut u8;
            unsafe {
                if write_to_user {
                    core::ptr::copy(buffer.add(offset), physical.add(within), count);
                } else {
                    core::ptr::copy(physical.add(within), buffer.add(offset), count);
                }
            }
            offset += count;
        }
        true
    }
}
