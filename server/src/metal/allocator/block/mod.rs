use crate::metal::allocator::InitResult;
use crate::metal::sync::OnceLock;
use core::alloc::AllocError;
use core::alloc::Allocator;
use core::alloc::Layout;
use core::ptr::NonNull;
use erra::ResultExt;

use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::metal::allocator::InitError;
use kernel_api::model::cell::PAGE_SIZE;

mod metadata;

mod inner;
mod pool;
mod pump;
mod tally;

use metadata::BlockMeta;

use inner::BlockInner;
use pool::Pool;
use tally::Tally;

const MIN_POWER: usize = 3;
const MAX_POWER: usize = (PAGE_SIZE / 2).ilog2() as usize;
pub(crate) const MAX_SIZE: usize = 1 << MAX_POWER;
const PAGE_SHIFT: usize = PAGE_SIZE.ilog2() as usize;

fn power_of(size: usize) -> usize {
    size.max(1usize << MIN_POWER).next_power_of_two().ilog2() as usize
}

pub struct BlockAllocator {
    blocks: &'static [BlockInner],
    tally: &'static Tally,
}

impl BlockAllocator {
    pub fn new() -> Result<Self, InitError> {
        let nodes = crate::metal::hart::hart_count();
        assert!(nodes > 0, "block init: no harts");
        let m = crate::metal::board::info().memory.allocation_bounds();

        let tally = {
            let meta_len = m.size.div_ceil(PAGE_SIZE);
            let meta = crate::metal::allocator::bump::allocator()
                .allocate(Layout::array::<BlockMeta>(meta_len).map_err(|_| InitError::OutOfMemory)?)
                .map_err(|_| InitError::OutOfMemory)?;
            let cells = meta.as_ptr() as *mut BlockMeta;
            unsafe {
                for i in 0..meta_len {
                    cells.add(i).write(BlockMeta::free());
                }
            }
            Box::leak(
                Box::try_new(Tally::new(m.base, cells, meta_len))
                    .map_err(|_| InitError::OutOfMemory)?,
            )
        };
        crate::metal::allocator::statistics::install_block_kinds(m.base, m.size)?;

        let mut pools = Vec::new();
        pools
            .try_reserve_exact(nodes)
            .map_err(|_| InitError::OutOfMemory)?;
        for i in 0..nodes {
            let mut pool = Pool::new();
            pool.init()?;
            pools.push(BlockInner::new(i, tally, pool));
        }

        Ok(BlockAllocator {
            blocks: Box::leak(pools.into_boxed_slice()),
            tally,
        })
    }

    /// 页所属的池下标；越界或未归属返回 `None`。这是池归属读数，不是授权。
    pub fn pool_of(&self, pa: usize) -> Option<usize> {
        self.tally.owner_of(pa)
    }

    /// 每池先抽干泵，再清掉本池的空闲页与 spare 迟滞。
    pub fn flush(&self) {
        for pool in self.blocks {
            pool.suck();
            pool.clear();
        }
    }
}

unsafe impl Allocator for BlockAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        crate::metal::allocator::assert_allocation_allowed();
        let power = power_of(layout.size());
        if power > MAX_POWER || layout.align() > (1usize << power) {
            return Err(AllocError);
        }
        let me = crate::metal::hart::hart_id().get();
        let pool = &self.blocks[me];
        let addr = pool.pull(power).ok_or(AllocError)?;
        crate::metal::allocator::statistics::record_block_take(addr, power);

        Ok(NonNull::slice_from_raw_parts(
            unsafe { NonNull::new_unchecked(addr as *mut u8) },
            layout.size(),
        ))
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        let power = power_of(layout.size());
        let pa = ptr.addr().get();
        let Some(home) = self.pool_of(pa) else { return };
        crate::metal::allocator::statistics::record_block_give(pa, power);

        let me = crate::metal::hart::hart_id().get();
        let pool = &self.blocks[home];
        if home == me {
            pool.push(ptr, power);
        } else {
            pool.feed(ptr, power);
        }
    }
}

static BLOCK_ALLOCATOR: OnceLock<BlockAllocator> = OnceLock::new();

pub(crate) fn heap() -> &'static BlockAllocator {
    BLOCK_ALLOCATOR.get().expect("block heap not initialized")
}

pub(crate) fn flush() {
    heap().flush();
}

pub fn init() -> InitResult<()> {
    (|| -> Result<(), InitError> {
        let heap = BlockAllocator::new()?;
        BLOCK_ALLOCATOR
            .set(heap)
            .map_err(|_| InitError::AlreadyInitialized)
    })()
    .annotate("initializing block allocator")
}
