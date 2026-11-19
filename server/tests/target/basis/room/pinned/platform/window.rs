//! QEMU virt's identity-translated, bus-zero, 32-bit PCI aperture.
use kernel_api::abi::call::pend::PendFail;

pub(crate) fn bar_window(tree: &fdt::Fdt, host: usize) -> Result<(usize, usize), PendFail> {
    let mut selected = None;
    for node in tree.all_nodes() {
        if node.property("iommu-map").is_some()
            && node
                .reg()
                .and_then(|mut r| r.next())
                .is_some_and(|r| r.starting_address.addr() == host)
            && selected.replace(node).is_some()
        {
            return Err(PendFail::Denied);
        }
    }
    let node = selected.ok_or(PendFail::Denied)?;
    let property = |name| node.property(name).map(|p| p.value).ok_or(PendFail::Denied);
    let bus = property("bus-range")?;
    if bus.len() != 8
        || bus[..4] != [0, 0, 0, 0]
        || property("#address-cells")? != 3u32.to_be_bytes()
        || property("#size-cells")? != 2u32.to_be_bytes()
    {
        return Err(PendFail::Denied);
    }
    let ranges = property("ranges")?;
    if !ranges.len().is_multiple_of(28) {
        return Err(PendFail::Denied);
    }
    let mut window = None;
    for range in ranges.chunks_exact(28) {
        let word = |at: usize| u32::from_be_bytes(range[at * 4..at * 4 + 4].try_into().unwrap());
        if word(0) != 0x02000000 {
            continue;
        }
        let child = ((word(1) as u64) << 32) | word(2) as u64;
        let parent = ((word(3) as u64) << 32) | word(4) as u64;
        let size = ((word(5) as u64) << 32) | word(6) as u64;
        let end = parent.checked_add(size).ok_or(PendFail::Denied)?;
        if child != parent || parent == 0 || size == 0 || end > 1u64 << 32 || window.is_some() {
            return Err(PendFail::Denied);
        }
        window = Some((parent as usize, end as usize));
    }
    window.ok_or(PendFail::Denied)
}
