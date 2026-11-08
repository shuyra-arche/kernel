//! Bounded, allocation-free CPU admission. Slot zero always owns the boot hart.
mod capability;
mod property;

use crate::metal::hart::{FirmwareId, HartId};
use capability::Capability;
pub(crate) use capability::Paging;
use property::{enabled, number, property, string};

// Keep early facts and debug-build moves within the 64 KiB ROOT stack.
const CAPACITY: usize = 256;

#[derive(fack::prelude::Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum HartError {
    #[error("missing or malformed CPU property: {0}")]
    Property(&'static str),
    #[error("more than 256 CPU nodes")]
    Capacity,
    #[error("duplicate or reserved firmware hart ID: {0}")]
    Identity(usize),
    #[error("boot hart is absent or disabled: {0}")]
    Boot(usize),
    #[error("hart {0} has no supported MMU mode")]
    Mmu(usize),
    #[error("hart {0} lacks the RV64GC/CSR/fence/counter baseline")]
    Isa(usize),
}

#[derive(Debug, Clone, Copy)]
struct Cpu {
    id: FirmwareId,
    node: usize,
}

#[derive(Debug)]
pub struct HartInfo {
    cpus: [Cpu; CAPACITY],
    float: [u64; CAPACITY / 64],
    vector: [u64; CAPACITY / 64],
    sstc: [u64; CAPACITY / 64],
    pub count: usize,
    pub hertz: usize,
    pub(crate) common: Capability,
}

impl HartInfo {
    pub(crate) fn parse(fdt: &fdt::Fdt, boot: FirmwareId) -> Result<Self, HartError> {
        if fdt
            .find_node("/")
            .ok_or(HartError::Property("/"))?
            .children()
            .filter(|node| node.name == "cpus")
            .count()
            != 1
        {
            return Err(HartError::Property("/cpus"));
        }
        let cpus = fdt.find_node("/cpus").ok_or(HartError::Property("/cpus"))?;
        let width = number(cpus, "#address-cells")?;
        if !matches!(width, 1 | 2) || number(cpus, "#size-cells")? != 0 {
            return Err(HartError::Property("CPU cell widths"));
        }
        if !enabled(cpus)? {
            return Err(HartError::Boot(boot.get()));
        }
        let hertz = number(cpus, "timebase-frequency")?;
        if hertz == 0 {
            return Err(HartError::Property("timebase-frequency"));
        }
        let mut result = Self {
            cpus: [Cpu {
                id: FirmwareId::new(0),
                node: 0,
            }; CAPACITY],
            float: [0; CAPACITY / 64],
            vector: [0; CAPACITY / 64],
            sstc: [0; CAPACITY / 64],
            count: 0,
            hertz,
            common: Capability::ceiling(),
        };
        let mut seen = [0usize; CAPACITY];
        for (index, node) in cpus.children().filter(is_cpu).enumerate() {
            if index == CAPACITY {
                return Err(HartError::Capacity);
            }
            if string(node, "device_type")? != "cpu" {
                return Err(HartError::Property("device_type"));
            }
            let reg = property(node, "reg")?.ok_or(HartError::Property("reg"))?;
            if reg.len() != width * 4 {
                return Err(HartError::Property("reg"));
            }
            let id = number(node, "reg")?;
            if id == usize::MAX || seen[..index].contains(&id) {
                return Err(HartError::Identity(id));
            }
            seen[index] = id;
            if !enabled(node)? {
                continue;
            }
            let caps = Capability::parse(node, id)?;
            result.common.intersect(caps);
            result.cpus[result.count] = Cpu {
                id: FirmwareId::new(id),
                node: node.name.as_ptr() as usize,
            };
            if caps.floating() {
                result.float[result.count / 64] |= 1 << (result.count % 64);
            }
            if caps.vector() {
                result.vector[result.count / 64] |= 1 << (result.count % 64);
            }
            if caps.sstc() {
                result.sstc[result.count / 64] |= 1 << (result.count % 64);
            }
            result.count += 1;
        }
        let boot_slot = result.cpus[..result.count]
            .iter()
            .position(|cpu| cpu.id == boot)
            .ok_or(HartError::Boot(boot.get()))?;
        result.cpus.swap(0, boot_slot);
        for bitmap in [&mut result.float, &mut result.vector, &mut result.sstc] {
            let first = bitmap[0] & 1;
            let boot = bitmap[boot_slot / 64] >> (boot_slot % 64) & 1;
            if first != boot {
                bitmap[0] ^= 1;
                bitmap[boot_slot / 64] ^= 1 << (boot_slot % 64);
            }
        }
        Ok(result)
    }

    /// Membership of an already-admitted CPU node in the same immutable DTB.
    /// The stored address is compared only; it is never dereferenced.
    pub(crate) fn admitted(&self, node: fdt::node::FdtNode) -> Option<FirmwareId> {
        self.cpus[..self.count]
            .iter()
            .find_map(|cpu| (cpu.node == node.name.as_ptr() as usize).then_some(cpu.id))
    }

    pub(crate) fn sstc(&self, slot: HartId) -> bool {
        self.sstc[slot.get() / 64] & (1 << (slot.get() % 64)) != 0
    }

    pub(crate) fn vector(&self, slot: HartId) -> bool {
        self.vector[slot.get() / 64] & (1 << (slot.get() % 64)) != 0
    }

    pub(crate) fn floating(&self, slot: HartId) -> bool {
        self.float[slot.get() / 64] & (1 << (slot.get() % 64)) != 0
    }

    pub fn firmware(&self, slot: HartId) -> FirmwareId {
        assert!(slot.get() < self.count, "unregistered hart slot");
        self.cpus[slot.get()].id
    }
}

fn is_cpu(node: &fdt::node::FdtNode<'_, '_>) -> bool {
    node.name.split('@').next() == Some("cpu")
}
