//! Restricted QEMU PLIC route, parsed completely before any register effect.
use crate::metal::board::{
    harts::HartInfo,
    tree::{self, Error, Identities, mmio, property, scalar},
};
use crate::metal::hart::HartId;
use alloc::vec::Vec;

pub(crate) struct Plan {
    pub base: usize,
    pub size: usize,
    pub sources: usize,
    pub contexts: Vec<usize>,
    pub selected: usize,
    /// CQ, FQ, PQ. Performance monitoring is outside this mechanism.
    pub queues: [u32; 3],
}
impl Plan {
    pub(crate) fn discover(
        tree: &fdt::Fdt,
        harts: &HartInfo,
        boot: HartId,
        controller: usize,
    ) -> Result<Self, Error> {
        if boot.get() >= harts.count {
            return Err(Error::Description);
        }
        let boot = harts.firmware(boot);
        let identities = Identities::discover(tree)?;
        let mut controllers = tree.all_nodes().filter(|n| {
            n.compatible().is_some_and(|c| {
                c.all()
                    .any(|v| matches!(v, "riscv,plic0" | "sifive,plic-1.0.0"))
            })
        });
        let plic = controllers.next().ok_or(Error::Description)?;
        if controllers.next().is_some() || scalar(plic, "#interrupt-cells")? != Some(1) {
            return Err(Error::Description);
        }
        if !tree::enabled(tree, plic)? {
            return Err(Error::Description);
        }
        let phandle = identities.id(plic).ok_or(Error::Description)?;
        let sources = scalar(plic, "riscv,ndev")?.ok_or(Error::Description)? as usize;
        if !(1..=1023).contains(&sources) {
            return Err(Error::Description);
        }
        let region = mmio::single(tree, plic)?;
        let (base, size) = (region.base, region.size);
        if base == 0
            || !base.is_multiple_of(4096)
            || size < 0x201000
            || base.checked_add(size).is_none()
        {
            return Err(Error::Description);
        }
        let extended = property(plic, "interrupts-extended")?.ok_or(Error::Description)?;
        if extended.is_empty() || !extended.len().is_multiple_of(8) {
            return Err(Error::Description);
        }
        let cpus = tree.find_node("/cpus").ok_or(Error::Description)?;
        let mut contexts = Vec::new();
        contexts
            .try_reserve(extended.len() / 8)
            .map_err(|_| Error::OutOfMemory)?;
        let mut selected = None;
        for (index, pair) in extended.chunks_exact(8).enumerate() {
            let target = u32::from_be_bytes(pair[..4].try_into().unwrap()) as usize;
            let cause = u32::from_be_bytes(pair[4..].try_into().unwrap());
            let intc = identities
                .resolve(target as u32)
                .ok_or(Error::Description)?;
            if !intc
                .compatible()
                .is_some_and(|c| c.all().any(|c| c == "riscv,cpu-intc"))
                || property(intc, "interrupt-controller")? != Some(&[])
                || scalar(intc, "#interrupt-cells")? != Some(1)
            {
                return Err(Error::Description);
            }
            let cpu = tree::parent(tree, intc).ok_or(Error::Description)?;
            if cpu.name.split('@').next() != Some("cpu")
                || !tree::parent(tree, cpu)
                    .is_some_and(|parent| core::ptr::eq(parent.name, cpus.name))
            {
                return Err(Error::Description);
            }
            if cause != 9 {
                continue;
            }
            if 0x200000usize
                .checked_add(index.checked_mul(0x1000).ok_or(Error::Description)?)
                .and_then(|v| v.checked_add(8))
                .is_none_or(|end| end > size)
            {
                return Err(Error::Description);
            }
            contexts.push(index);
            if harts.admitted(cpu) == Some(boot) && tree::enabled(tree, intc)? {
                if selected.replace(index).is_some() {
                    return Err(Error::Description);
                }
            }
        }
        let mut selected_iommu = None;
        for node in tree.all_nodes().filter(|n| {
            n.compatible()
                .is_some_and(|c| c.all().any(|c| c == "riscv,iommu"))
        }) {
            if mmio::single(tree, node)?.base == controller
                && selected_iommu.replace(node).is_some()
            {
                return Err(Error::Description);
            }
        }
        let iommu = selected_iommu.ok_or(Error::Description)?;
        if !tree::enabled(tree, iommu)? || scalar(iommu, "interrupt-parent")? != Some(phandle) {
            return Err(Error::Description);
        }
        let spec = property(iommu, "interrupts")?.ok_or(Error::Description)?;
        // QEMU 10 virt emits (IRQ, edge-rising=1) pairs for this device even
        // though its PLIC declares one interrupt cell. Accept only this known
        // encoding or the standard four one-cell specifiers; never guess stride.
        let stride = match spec.len() {
            16 => 4,
            32 if spec.chunks_exact(8).all(|v| v[4..] == [0, 0, 0, 1]) => 8,
            _ => return Err(Error::Description),
        };
        let words: [u32; 4] = core::array::from_fn(|i| {
            u32::from_be_bytes(spec[i * stride..i * stride + 4].try_into().unwrap())
        });
        if words
            .iter()
            .enumerate()
            .any(|(i, irq)| *irq == 0 || *irq as usize > sources || words[..i].contains(irq))
        {
            return Err(Error::Description);
        }
        Ok(Self {
            base,
            size,
            sources,
            contexts,
            selected: selected.ok_or(Error::Description)?,
            queues: [words[0], words[1], words[3]],
        })
    }
}
