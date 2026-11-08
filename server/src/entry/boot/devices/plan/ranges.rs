use crate::machine::board::tree::{self, mmio};
use kernel_api::abi::call::pend::PendFail;

/// 是不是 PCI 宿主：`device_type` 必须**正好**是 `pci`。
///
/// 名字叫什么都可以，不按名字认；`pcie`、`pci-something` 这些前缀相同的取值也不收——
/// 它们的 `ranges` 未必是 PCI 宿主那一套语意。
pub(crate) fn is_pci_host(node: fdt::node::FdtNode) -> bool {
    node.property("device_type")
        .and_then(|property| property.as_str())
        == Some("pci")
}

/// PCI 宿主的 `ranges` 里那几段**内存空间**窗口的父地址与长度。
///
/// 一条占 `child + parent + size` 格（都是**四字节格**）：
/// 子地址那几格（`child` = 宿主自己的 `#address-cells`，PCI 通常 3——
/// 首格是空间码）→ 父地址那几格（`parent` = 父节点的 `#address-cells`）
/// → 长度那几格（`size` = 宿主自己的 `#size-cells`）。
/// 只取内存空间（空间码位 25:24 为 10 / 11）；IO 空间不是可映射的 MMIO 窗口。
/// 形状不是这一套（格数不合、字节长不成整条、坐标或跨度溢出）就报错，
/// 不静默退回空表。
pub(crate) fn ranges_windows(
    value: &[u8],
    child: usize,
    parent: usize,
    size: usize,
) -> Result<alloc::vec::Vec<(usize, usize)>, PendFail> {
    if child != 3 || !(1..=2).contains(&parent) || !(1..=2).contains(&size) {
        return Err(PendFail::Denied);
    }
    let width = child + parent + size;
    if !value.len().is_multiple_of(width * 4) {
        return Err(PendFail::Denied);
    }
    let count = value.len() / (width * 4);
    let mut windows = alloc::vec::Vec::new();
    windows
        .try_reserve(count)
        .map_err(|_| PendFail::OutOfMemory)?;
    for entry in value.chunks_exact(width * 4) {
        let cell = |at: usize| -> u64 {
            let bytes: [u8; 4] = entry[at * 4..at * 4 + 4].try_into().expect("whole cell");
            u32::from_be_bytes(bytes) as u64
        };
        // 空间码在子地址的首格；不是内存空间就不是可映射窗口。
        if (cell(0) >> 24) & 0x3 < 2 {
            continue;
        }
        let mut address = 0u64;
        for at in child..child + parent {
            address = address.checked_shl(32).ok_or(PendFail::Denied)? | cell(at);
        }
        let mut length = 0u64;
        for at in child + parent..width {
            length = length.checked_shl(32).ok_or(PendFail::Denied)? | cell(at);
        }
        let base = usize::try_from(address).map_err(|_| PendFail::Denied)?;
        let span = usize::try_from(length).map_err(|_| PendFail::Denied)?;
        if base == 0 || span == 0 {
            continue;
        }
        base.checked_add(span).ok_or(PendFail::Denied)?;
        windows.push((base, span));
    }
    Ok(windows)
}

pub(crate) fn memory_windows(
    fdt: &fdt::Fdt,
    node: fdt::node::FdtNode,
) -> Result<alloc::vec::Vec<(usize, usize)>, PendFail> {
    let Some(ranges) = tree::property(node, "ranges")? else {
        return Ok(alloc::vec::Vec::new());
    };
    let child = tree::cells(node, "#address-cells", 0)?;
    let size = tree::cells(node, "#size-cells", 0)?;
    let parent = tree::parent(fdt, node).ok_or(PendFail::Denied)?;
    let addresses = tree::cells(parent, "#address-cells", 0)?;
    let mut windows = ranges_windows(ranges, child, addresses, size)?;
    for (base, size) in &mut windows {
        let physical = mmio::physical(fdt, parent, *base, *size)?;
        *base = physical.base;
        *size = physical.size;
    }
    Ok(windows)
}
