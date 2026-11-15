//! One allocator entry, with independent Buddy state for each physical region.
use crate::metal::allocator::{InitError, pages::Pages};
use alloc::vec::Vec;
use kernel_api::address::PAGE_SIZE;

#[path = "buddy.rs"]
mod buddy;
use buddy::Buddy;

pub(super) struct FrameInner {
    regions: Vec<Buddy>,
    pages: Pages<'static>,
    pub(super) capacity: usize,
    pub(super) allocated: usize,
}
impl FrameInner {
    pub(super) fn new() -> Self {
        Self {
            regions: Vec::new(),
            pages: Pages::new(&[]),
            capacity: 0,
            allocated: 0,
        }
    }
    pub(super) fn init(&mut self) -> Result<(), InitError> {
        let memory = &crate::entry::boot::facts::info().memory;
        self.pages = Pages::new(memory.free());
        self.regions
            .try_reserve_exact(memory.free().len())
            .map_err(|_| InitError::OutOfMemory)?;
        for region in memory.free() {
            self.regions.push(Buddy::new(region.base, region.size)?);
        }
        crate::metal::allocator::statistics::install_frame_kinds(self.pages.len())?;
        let frontier = crate::metal::allocator::bump::frontier()
            .checked_next_multiple_of(PAGE_SIZE)
            .ok_or(InitError::NoFreeFrames)?;
        for (region, buddy) in memory.free().iter().zip(&mut self.regions) {
            let start = if region.base == memory.bootstrap().base {
                frontier
            } else {
                region.base
            };
            self.capacity += unsafe { buddy.publish(start) };
        }
        if self.capacity == 0 {
            return Err(InitError::NoFreeFrames);
        }
        Ok(())
    }
    pub(super) fn frame_index(&self, address: usize) -> usize {
        self.pages
            .index(address)
            .expect("frame outside admitted RAM regions")
    }
    pub(super) fn frame_addr(&self, index: usize) -> usize {
        self.pages
            .address(index)
            .expect("invalid compact frame index")
    }
    pub(super) unsafe fn split_block(&mut self, power: usize) -> Option<usize> {
        let mut offset = 0;
        for region in &mut self.regions {
            if let Some(index) = unsafe { region.split_block(power) } {
                return Some(offset + index);
            }
            offset += region.pages();
        }
        None
    }
    pub(super) unsafe fn merge_block(&mut self, mut index: usize, power: usize) {
        for region in &mut self.regions {
            if index < region.pages() {
                assert!(
                    index
                        .checked_add(1 << power)
                        .is_some_and(|end| end <= region.pages()),
                    "receipt crosses physical region"
                );
                unsafe { region.merge_block(index, power) };
                return;
            }
            index -= region.pages();
        }
        panic!("invalid compact frame receipt");
    }
}
