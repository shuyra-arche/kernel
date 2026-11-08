use crate::machine::mmu::riscv::table::PteFlags;
use crate::machine::mmu::riscv::table::TableNode;
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
        let edge = crate::machine::board::dram_edge().unwrap_or(0x9000_0000);
        let (base, flags) = TableNode::walk_raw(
            self.root,
            crate::machine::mmu::riscv::canonical(page),
            |pa| (0x8000_0000..edge).contains(&pa.as_usize()),
        )?;
        if !flags.contains(PteFlags::R) {
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
