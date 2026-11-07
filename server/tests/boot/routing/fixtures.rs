use crate::{topology::Topology, tree::Node};

pub const CONTROLLER: usize = 0x3010000;
pub const HOST: usize = 0x30000000;

pub fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}

pub fn controller(base: usize, phandle: u32) -> Node {
    Node::new("iommu")
        .compatible("riscv,iommu")
        .property("phandle", phandle.to_be_bytes())
        .property("#iommu-cells", 1u32.to_be_bytes())
        .reg(base as u64, 4096)
}

pub fn host(base: usize, entries: &[u32]) -> Node {
    Node::new("pci")
        .compatible("pci-host-ecam-generic")
        .property("device_type", b"pci\0")
        .property("bus-range", words(&[0, 255]))
        .property("iommu-map", words(entries))
        .reg(base as u64, 0x10000000)
}

pub fn root() -> Node {
    Node::new("").cells()
}

pub fn discover(root: Node) -> Result<Topology, crate::topology::RouteError> {
    let bytes = root.encode();
    Topology::discover(&fdt::Fdt::new(&bytes).unwrap())
}
