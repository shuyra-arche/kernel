use core::ptr::NonNull;

use alloc::vec::Vec;

use crate::metal::allocator::InitError;
use crate::metal::allocator::Link;
use kernel_api::model::cell::PAGE_SIZE;

static STALE_RM: ::core::sync::atomic::AtomicUsize = ::core::sync::atomic::AtomicUsize::new(0);

pub(super) struct FrameInner {
    freelist: Vec<Option<NonNull<Link>>>,
    pagemeta: Vec<Option<u8>>,
    pub(super) capacity: usize,
    pub(super) allocated: usize,
    pub(super) base: usize,
    pub(super) edge: usize,
}

// SAFETY: 裸指针只指向同一块受 Frame 锁保护的物理帧，且只在 guard 内读写。
unsafe impl Send for FrameInner {}

impl FrameInner {
    pub(super) const fn new() -> Self {
        Self {
            freelist: Vec::new(),
            pagemeta: Vec::new(),
            capacity: 0,
            allocated: 0,
            base: 0,
            edge: 0,
        }
    }

    pub(super) fn init(&mut self) -> Result<(), InitError> {
        let memory = &crate::entry::boot::facts::info().memory;
        let bounds = memory.allocation_bounds();
        self.base = bounds.base;
        self.edge = bounds.range().end;
        let max_frame = bounds.size / PAGE_SIZE;
        let max_power = max_frame.ilog2() as usize + 1;
        self.freelist
            .try_reserve_exact(max_power)
            .map_err(|_| InitError::OutOfMemory)?;
        self.freelist.resize_with(max_power, || None);
        self.pagemeta
            .try_reserve_exact(max_frame)
            .map_err(|_| InitError::OutOfMemory)?;
        self.pagemeta.resize_with(max_frame, || None);
        crate::metal::allocator::statistics::install_frame_kinds(max_frame)?;

        // Metadata was allocated only inside the validated bootstrap interval.
        // Publish free links only after the final Bump frontier is known.
        let bootstrap = memory.bootstrap();
        let frontier = crate::metal::allocator::bump::frontier()
            .checked_next_multiple_of(PAGE_SIZE)
            .ok_or(InitError::NoFreeFrames)?;
        let mut available = false;
        for region in memory.free() {
            let mut start = region.base;
            if start == bootstrap.base {
                start = frontier;
            }
            let end = region.range().end;
            while start < end {
                let pages = (end - start) / PAGE_SIZE;
                let power =
                    ((start / PAGE_SIZE).trailing_zeros() as usize).min(pages.ilog2() as usize);
                unsafe {
                    self.push_link(self.frame_index(start), power);
                }
                self.capacity += PAGE_SIZE << power;
                start += PAGE_SIZE << power;
                available = true;
            }
        }
        if !available {
            return Err(InitError::NoFreeFrames);
        }
        Ok(())
    }

    pub(super) fn frame_index(&self, addr: usize) -> usize {
        (addr - self.base) / PAGE_SIZE
    }

    pub(super) fn frame_addr(&self, index: usize) -> usize {
        self.base + index * PAGE_SIZE
    }

    fn buddy_index(&self, index: usize, power: usize) -> Option<usize> {
        let page = self.base / PAGE_SIZE + index;
        (page ^ (1 << power))
            .checked_sub(self.base / PAGE_SIZE)
            .filter(|buddy| *buddy < self.pagemeta.len())
    }

    fn clear_head(&mut self, index: usize) {
        self.pagemeta[index] = None;
    }

    unsafe fn pull_link(&mut self, power: usize) -> Option<usize> {
        unsafe {
            let head = self.freelist[power]?;

            let addr = head.addr().get();
            let index = self.frame_index(addr);
            if index >= self.pagemeta.len() {
                static BAD: ::core::sync::atomic::AtomicUsize =
                    ::core::sync::atomic::AtomicUsize::new(0);
                let n = BAD.fetch_add(1, ::core::sync::atomic::Ordering::Relaxed) + 1;
                if n <= 8 {
                    crate::metal::console::_write(format_args!(
                        "frame freelist corrupt: pop index={index:#x} len={} power={power} ({n})",
                        self.pagemeta.len()
                    ));
                }
                return None;
            }

            let next = head.read().next;
            self.freelist[power] = next;
            if let Some(n) = next {
                (*n.as_ptr()).prev = None;
            }

            self.clear_head(index);
            Some(index)
        }
    }

    unsafe fn push_link(&mut self, index: usize, power: usize) {
        unsafe {
            let addr = NonNull::new_unchecked(self.frame_addr(index) as *mut Link);
            addr.write(Link::new(None, self.freelist[power]));

            if let Some(head) = self.freelist[power] {
                (*head.as_ptr()).prev = Some(addr);
            }

            self.freelist[power] = Some(addr);
            self.pagemeta[index] = Some(power as u8);
        }
    }

    unsafe fn remove_link(&mut self, index: usize, power: usize) {
        unsafe {
            let addr = self.frame_addr(index) as *mut Link;

            let prev = (*addr).prev;
            let next = (*addr).next;

            if let Some(p) = prev {
                (*p.as_ptr()).next = next;
            } else {
                let head = self.freelist[power].map(|h| h.as_ptr() as usize);
                if head != Some(addr as usize) {
                    let n = STALE_RM.fetch_add(1, ::core::sync::atomic::Ordering::Relaxed) + 1;
                    if n <= 8 {
                        crate::metal::console::_write(format_args!(
                            "[stale-rm] remove_link idx={index} power={power} 自称桶头但桶头={:?} next={:?}（第 {n} 次）",
                            head.map(|h| (h - self.base) / PAGE_SIZE),
                            next.map(|x| (x.as_ptr() as usize - self.base) / PAGE_SIZE)
                        ));
                    }
                }
                self.freelist[power] = next;
            }
            if let Some(n) = next {
                (*n.as_ptr()).prev = prev;
            }
        }
    }

    pub(super) unsafe fn split_block(&mut self, power: usize) -> Option<usize> {
        unsafe {
            let mut k = power;
            while k < self.freelist.len() && self.freelist[k].is_none() {
                k += 1;
            }
            if k >= self.freelist.len() {
                return None;
            }

            let index = self.pull_link(k)?;

            while k > power {
                k -= 1;
                let buddy = self
                    .buddy_index(index, k)
                    .expect("split stays within admitted block");
                self.push_link(buddy, k);
            }

            Some(index)
        }
    }

    pub(super) unsafe fn merge_block(&mut self, mut index: usize, mut power: usize) {
        unsafe {
            self.clear_head(index);
            while power < self.freelist.len() {
                let Some(buddy) = self.buddy_index(index, power) else {
                    break;
                };

                if buddy >= self.pagemeta.len() {
                    break;
                }

                if !self.pagemeta[buddy].is_some_and(|p| p as usize == power) {
                    break;
                }
                if !self.in_freelist(buddy, power) {
                    break;
                }

                self.remove_link(buddy, power);
                self.clear_head(buddy);
                index = index.min(buddy);
                power += 1;
            }

            self.push_link(index, power);
            if !self.in_freelist(index, power) {
                static LOST: ::core::sync::atomic::AtomicUsize =
                    ::core::sync::atomic::AtomicUsize::new(0);
                let n = LOST.fetch_add(1, ::core::sync::atomic::Ordering::Relaxed) + 1;
                if n <= 8 {
                    crate::metal::console::_write(format_args!(
                        "merge: pushed block NOT in chain idx={index} power={power} ({n})"
                    ));
                }
            }
        }
    }

    fn in_freelist(&self, index: usize, power: usize) -> bool {
        let target = self.frame_addr(index);
        let mut cur = self.freelist[power];
        while let Some(node) = cur {
            if node.as_ptr() as usize == target {
                return true;
            }
            cur = unsafe { node.read() }.next;
        }
        false
    }
}
