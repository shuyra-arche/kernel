//! Restricted QEMU PLIC route, parsed completely before any register effect.
use alloc::vec::Vec;
use fdt::node::FdtNode;

pub(crate) struct Plan {
    pub base: usize,
    pub size: usize,
    pub sources: usize,
    pub contexts: Vec<usize>,
    pub selected: usize,
    /// CQ, FQ, PQ. Performance monitoring is outside this mechanism.
    pub queues: [u32; 3],
}
fn number(node: FdtNode, name: &str) -> Result<usize, ()> {
    node.property(name)
        .filter(|p| p.value.len() == 4)
        .and_then(|p| p.as_usize())
        .ok_or(())
}
impl Plan {
    pub(crate) fn discover(tree: &fdt::Fdt, boot: usize, controller: usize) -> Result<Self, ()> {
        let mut controllers = tree.all_nodes().filter(|n| {
            n.compatible().is_some_and(|c| {
                c.all()
                    .any(|v| matches!(v, "riscv,plic0" | "sifive,plic-1.0.0"))
            })
        });
        let plic = controllers.next().ok_or(())?;
        if controllers.next().is_some() || number(plic, "#interrupt-cells")? != 1 {
            return Err(());
        }
        if plic
            .property("status")
            .is_some_and(|p| !matches!(p.as_str(), Some("okay" | "ok")))
        {
            return Err(());
        }
        let phandle = number(plic, "phandle")?;
        let sources = number(plic, "riscv,ndev")?;
        if !(1..=1023).contains(&sources) {
            return Err(());
        }
        let mut regions = plic.reg().ok_or(())?;
        let region = regions.next().ok_or(())?;
        if regions.next().is_some() {
            return Err(());
        }
        let base = region.starting_address.addr();
        let size = region.size.ok_or(())?;
        if base == 0
            || !base.is_multiple_of(4096)
            || size < 0x201000
            || base.checked_add(size).is_none()
        {
            return Err(());
        }
        let extended = plic.property("interrupts-extended").ok_or(())?.value;
        if extended.is_empty() || !extended.len().is_multiple_of(8) {
            return Err(());
        }
        let cpus = tree.find_node("/cpus").ok_or(())?;
        let mut contexts = Vec::new();
        contexts.try_reserve(extended.len() / 8).map_err(|_| ())?;
        let mut selected = None;
        for (index, pair) in extended.chunks_exact(8).enumerate() {
            let target = u32::from_be_bytes(pair[..4].try_into().unwrap()) as usize;
            let cause = u32::from_be_bytes(pair[4..].try_into().unwrap());
            let cpu = cpus
                .children()
                .find(|cpu| {
                    cpu.children().any(|intc| {
                        number(intc, "phandle") == Ok(target)
                            && number(intc, "#interrupt-cells") == Ok(1)
                    })
                })
                .ok_or(())?;
            if cause != 9 {
                continue;
            }
            if 0x200000usize
                .checked_add(index.checked_mul(0x1000).ok_or(())?)
                .and_then(|v| v.checked_add(8))
                .is_none_or(|end| end > size)
            {
                return Err(());
            }
            contexts.push(index);
            if cpu.property("reg").and_then(|p| p.as_usize()) == Some(boot) {
                if selected.replace(index).is_some() {
                    return Err(());
                }
            }
        }
        let iommu = tree
            .all_nodes()
            .find(|n| {
                n.compatible()
                    .is_some_and(|c| c.all().any(|v| v == "riscv,iommu"))
                    && n.reg().is_some_and(|mut r| {
                        r.next()
                            .is_some_and(|r| r.starting_address.addr() == controller)
                    })
            })
            .ok_or(())?;
        if number(iommu, "interrupt-parent")? != phandle {
            return Err(());
        }
        let spec = iommu.property("interrupts").ok_or(())?.value;
        // QEMU 10 virt emits (IRQ, edge-rising=1) pairs for this device even
        // though its PLIC declares one interrupt cell. Accept only this known
        // encoding or the standard four one-cell specifiers; never guess stride.
        let stride = match spec.len() {
            16 => 4,
            32 if spec.chunks_exact(8).all(|v| v[4..] == [0, 0, 0, 1]) => 8,
            _ => return Err(()),
        };
        let words: [u32; 4] = core::array::from_fn(|i| {
            u32::from_be_bytes(spec[i * stride..i * stride + 4].try_into().unwrap())
        });
        if words
            .iter()
            .enumerate()
            .any(|(i, irq)| *irq == 0 || *irq as usize > sources || words[..i].contains(irq))
        {
            return Err(());
        }
        Ok(Self {
            base,
            size,
            sources,
            contexts,
            selected: selected.ok_or(())?,
            queues: [words[0], words[1], words[3]],
        })
    }
}
