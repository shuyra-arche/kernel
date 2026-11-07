use core::alloc::AllocError;
use core::alloc::Layout;
use core::ptr::NonNull;

use kernel_api::model::cell::PAGE_SIZE;

use super::MAX_POWER;
use super::MIN_POWER;
use super::metadata::BlockMeta;
use super::pool::Pool;
use super::pump::Pump;
use super::tally::Tally;

pub(super) struct BlockInner {
    id: usize,
    tally: &'static Tally,
    pool: crate::machine::sync::SpinLock<Pool>,
    pump: [crate::machine::sync::SpinLock<Pump>; MAX_POWER + 1],
}

impl BlockInner {
    pub(super) fn new(id: usize, tally: &'static Tally, pool: Pool) -> BlockInner {
        BlockInner {
            id,
            tally,
            pool: crate::machine::sync::SpinLock::new(pool),
            pump: core::array::from_fn(|_| crate::machine::sync::SpinLock::new(Pump::new())),
        }
    }

    pub(super) fn pull(&self, power: usize) -> Option<usize> {
        self.suck();
        self.pool.with(|inner| {
            if let Some(head) = inner.freepool[power] {
                let page = head.as_ptr() as usize & !(PAGE_SIZE - 1);
                if inner.spare[power] == Some(page) {
                    inner.spare[power] = None;
                }
                let next = unsafe { head.cast::<Option<NonNull<u8>>>().read() };
                inner.freepool[power] = next;
                self.tally.inc_used(page);

                return Some(head.as_ptr() as usize);
            }
            let first = self.prime(inner, power).ok()?;
            Some(first.as_ptr() as usize)
        })
    }

    fn prime(&self, inner: &mut Pool, power: usize) -> Result<NonNull<u8>, AllocError> {
        let layout = Layout::from_size_align(PAGE_SIZE, PAGE_SIZE).unwrap();
        let page = crate::tag!(
            Prime,
            crate::machine::allocator::frame::allocator().allocate(layout)
        )
        .map_err(|_| AllocError)?;
        crate::machine::allocator::statistics::record_pool_take();
        let base = page.as_ptr() as *mut u8 as usize;

        self.tally.write(base, BlockMeta::new(self.id, power));
        let block_nums = PAGE_SIZE >> power;
        unsafe {
            {
                let block_size = 1usize << power;
                for i in 0..block_nums.saturating_sub(1) {
                    let this = base + i * block_size;
                    let next = base + (i + 1) * block_size;
                    NonNull::new_unchecked(this as *mut Option<NonNull<u8>>)
                        .write(Some(NonNull::new_unchecked(next as *mut u8)));
                }
                if block_nums > 0 {
                    NonNull::new_unchecked(
                        (base + (block_nums - 1) * block_size) as *mut Option<NonNull<u8>>,
                    )
                    .write(None);
                }
            };
            let first = NonNull::new_unchecked(base as *mut u8);
            inner.freepool[power] = first.cast::<Option<NonNull<u8>>>().read();
            first.cast::<Option<NonNull<u8>>>().write(None);
            Ok(first)
        }
    }

    fn drain(&self, inner: &mut Pool, power: usize, page: usize) {
        let mut keep: Option<NonNull<u8>> = None;
        let mut head = inner.freepool[power];
        let mut n = 0usize;
        while let Some(node) = head {
            n += 1;
            if n > 1 << 16 {
                panic!("block allocator: drain[{power}] walk exceeded depth — cyclic chain?");
            }
            let next = unsafe { node.cast::<Option<NonNull<u8>>>().read() };
            let addr = node.as_ptr() as usize;
            if addr >= page && addr < page + PAGE_SIZE {
            } else {
                unsafe {
                    node.cast::<Option<NonNull<u8>>>().write(keep);
                }
                keep = Some(node);
            }
            head = next;
        }
        inner.freepool[power] = keep;

        self.tally.write(page, BlockMeta::free());

        let layout = Layout::from_size_align(PAGE_SIZE, PAGE_SIZE).unwrap();
        unsafe {
            crate::machine::allocator::frame::allocator()
                .deallocate(NonNull::new_unchecked(page as *mut u8), layout)
        };
        crate::machine::allocator::statistics::record_pool_give();
    }

    pub(super) fn push(&self, ptr: NonNull<u8>, power: usize) {
        self.pool.with(|inner| {
            unsafe {
                ptr.cast::<Option<NonNull<u8>>>()
                    .write(inner.freepool[power]);
            }
            inner.freepool[power] = Some(ptr);

            let page = ptr.as_ptr() as usize & !(PAGE_SIZE - 1);
            let empty = self.tally.dec_used(page);
            if empty {
                if inner.spare[power].is_none() {
                    inner.spare[power] = Some(page);
                } else {
                    self.drain(inner, power, page);
                }
            }
        });
    }

    pub(super) fn feed(&self, ptr: NonNull<u8>, power: usize) {
        self.pump[power].with(|g| unsafe { g.push(ptr) });
    }

    pub(super) fn suck(&self) {
        for power in MIN_POWER..=MAX_POWER {
            let head = self.pump[power].with(|g| g.take());
            let mut this = head;
            let mut n = 0usize;
            while let Some(node) = this {
                n += 1;
                if n > 1 << 14 {
                    panic!(
                        "block allocator: pump[{power}] walk exceeded depth — cyclic chain (double feed?)"
                    );
                }
                let next = unsafe { node.cast::<Option<NonNull<u8>>>().read() };
                self.push(node, power);
                this = next;
            }
        }
    }

    pub(super) fn clear(&self) {
        self.pool.with(|inner| {
            for idx in 0..self.tally.len() {
                let m = self.tally.read_idx(idx);
                if m.owner() == Some(self.id) && m.used() == 0 {
                    let frame = self.tally.frame_of(idx);
                    self.drain(inner, m.power as usize, frame);
                }
            }
            inner.spare.iter_mut().for_each(|s| *s = None);
        });
    }
}
