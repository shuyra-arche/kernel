use crate::metal::mmu::table::PteFlags;
use crate::metal::mmu::table::TableNode;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::PhysAddr;

#[derive(Clone)]
pub struct StackReader {
    root: PhysAddr,
    page: Option<(usize, PhysAddr)>,
}

impl StackReader {
    pub fn new(root_ppn: usize) -> StackReader {
        StackReader {
            root: PhysAddr::from_raw(root_ppn << 12),
            page: None,
        }
    }

    fn leaf(&mut self, page: usize) -> Option<PhysAddr> {
        if let Some((cached, base)) = self.page
            && cached == page
        {
            return Some(base);
        }
        let (base, flags) =
            TableNode::walk_raw(self.root, crate::metal::mmu::canonical(page), |pa| {
                crate::entry::boot::facts::contains_identity(pa.as_usize(), PAGE_SIZE)
            })?;
        if !flags.contains(PteFlags::R)
            || !crate::entry::boot::facts::contains_identity(base.as_usize(), PAGE_SIZE)
        {
            return None;
        }
        self.page = Some((page, base));
        Some(base)
    }

    pub fn word(&mut self, addr: usize) -> Option<usize> {
        if (addr & (PAGE_SIZE - 1)) + size_of::<usize>() > PAGE_SIZE {
            return None;
        }
        let page = addr & !(PAGE_SIZE - 1);
        let base = self.leaf(page)?;
        Some(unsafe {
            (base.as_usize() as *const u8)
                .add(addr - page)
                .cast::<usize>()
                .read_unaligned()
        })
    }

    pub fn pair(&mut self, frame: usize) -> Option<(usize, usize)> {
        Some((
            self.word(frame.checked_sub(16)?)?,
            self.word(frame.checked_sub(8)?)?,
        ))
    }
}
