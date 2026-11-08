//! Bounded, allocation-free CPU admission. Slot zero always owns the boot hart.
mod capability;
mod property;

use crate::machine::hart::{FirmwareId, HartId};
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

#[derive(Debug)]
pub struct HartInfo {
    ids: [FirmwareId; CAPACITY],
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
            ids: [FirmwareId::new(0); CAPACITY],
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
            result.ids[result.count] = FirmwareId::new(id);
            result.count += 1;
        }
        let boot_slot = result.ids[..result.count]
            .iter()
            .position(|id| *id == boot)
            .ok_or(HartError::Boot(boot.get()))?;
        result.ids.swap(0, boot_slot);
        Ok(result)
    }

    pub fn firmware(&self, slot: HartId) -> FirmwareId {
        assert!(slot.get() < self.count, "unregistered hart slot");
        self.ids[slot.get()]
    }
}

fn is_cpu(node: &fdt::node::FdtNode<'_, '_>) -> bool {
    node.name.split('@').next() == Some("cpu")
}
