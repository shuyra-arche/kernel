use kernel_api::abi::call::pend::PendFail;

use super::plan::Plan;
use super::tree::Node;

const IOMMU: u64 = 0x301_0000;
const UART: u64 = 0x1000_0000;
const DRAM: usize = 0x8000_0000;

fn authorities(root: Node) -> Result<Vec<(usize, usize)>, PendFail> {
    let bytes = root.encode();
    let fdt = fdt::Fdt::new(&bytes).expect("valid fixture DTB");
    Plan::discover(&fdt, DRAM, 0x1000_0000).map(|plan| {
        plan.authorities()
            .iter()
            .map(|r| (r.base, r.size))
            .collect()
    })
}

fn root() -> Node {
    Node::new("").cells()
}

#[test]
fn prepared_external_resource_cannot_alias_other_authority_or_control() {
    use super::description::mmio::Region;
    let bytes = root()
        .child(iommu(IOMMU, 0x1000))
        .child(Node::new("uart@10000000").reg(UART, 0x100))
        .child(
            Node::new("timer@2000000")
                .compatible("riscv,clint0")
                .property("status", b"disabled\0")
                .reg(0x2000000, 0x10000),
        )
        .encode();
    let tree = fdt::Fdt::new(&bytes).unwrap();
    let plan = Plan::discover(&tree, DRAM, 0x1000_0000).unwrap();
    for base in [0, UART as usize, IOMMU as usize, 0x2000000] {
        assert_eq!(
            plan.external(&tree, Region::new(base, 0x1000).unwrap()),
            Err(PendFail::Denied)
        );
    }
    assert!(
        plan.external(&tree, Region::new(0x40000000, 0x1000).unwrap())
            .is_ok()
    );
}

fn iommu(base: u64, size: u64) -> Node {
    Node::new("controller@3010000")
        .compatible("qemu,riscv-iommu\0riscv,iommu")
        .property("#iommu-cells", 1u32.to_be_bytes())
        .reg(base, size)
}

fn pci(base: u64, size: u64) -> Node {
    Node::new("host@30000000")
        .property("device_type", b"pci\0")
        .property("#address-cells", 3u32.to_be_bytes())
        .property("#size-cells", 2u32.to_be_bytes())
        .reg(0x3000_0000, 0x1000_0000)
        .property(
            "ranges",
            [
                0x0300_0000,
                (base >> 32) as u32,
                base as u32,
                (base >> 32) as u32,
                base as u32,
                (size >> 32) as u32,
                size as u32,
            ]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>(),
        )
}

#[test]
fn platform_control_is_withheld_but_unrelated_windows_keep_their_names() {
    let tree = root()
        .child(
            Node::new("soc")
                .cells()
                .property("ranges", [])
                .child(iommu(IOMMU, 0x1000))
                .child(Node::new("uart@10000000").reg(UART, 0x100)),
        )
        .child(pci(0x4_0000_0000, 0x4_0000_0000));
    assert_eq!(
        authorities(tree).unwrap(),
        [
            (UART as usize, 0x100),
            (0x3000_0000, 0x1000_0000),
            (0x4_0000_0000, 0x4_0000_0000),
        ]
    );
}

#[test]
fn aliases_are_checked_after_discovery_regardless_of_node_order() {
    for controller_first in [false, true] {
        let alias = Node::new("other@3000000").reg(IOMMU - 0x10000, 0x20000);
        let controller = iommu(IOMMU, 0x1000);
        let tree = if controller_first {
            root().child(controller).child(alias)
        } else {
            root().child(alias).child(controller)
        };
        assert!(authorities(tree).unwrap().is_empty());
    }
}

#[test]
fn pci_aperture_cannot_reintroduce_control_registers() {
    let tree = root()
        .child(pci(IOMMU - 0x10000, 0x20000))
        .child(iommu(IOMMU, 0x1000));
    assert_eq!(authorities(tree).unwrap(), [(0x3000_0000, 0x1000_0000)]);
}

#[test]
fn exclusion_uses_page_coverage_including_neighboring_bytes() {
    let tree = root()
        .child(Node::new("before").reg(IOMMU, 1))
        .child(Node::new("after").reg(IOMMU + 0x180, 1))
        .child(Node::new("next-page").reg(IOMMU + 0x1000, 1))
        .child(iommu(IOMMU + 0x100, 0x80));
    assert_eq!(authorities(tree).unwrap(), [((IOMMU + 0x1000) as usize, 1)]);
}

#[test]
fn compatible_not_node_name_or_status_defines_control_pages() {
    let tree = root()
        .child(
            Node::new("iommu@10000000")
                .compatible("ns16550a")
                .reg(UART, 0x100),
        )
        .child(iommu(IOMMU, 0x1000).property("status", b"disabled\0"));
    assert_eq!(authorities(tree).unwrap(), [(UART as usize, 0x100)]);
}

#[test]
fn every_controller_and_reg_entry_is_reserved() {
    let regions = [IOMMU, 0x1000, IOMMU + 0x2000, 0x1000]
        .into_iter()
        .flat_map(u64::to_be_bytes)
        .collect::<Vec<_>>();
    let tree = root()
        .child(
            Node::new("multi")
                .compatible("riscv,iommu")
                .property("reg", regions),
        )
        .child(iommu(IOMMU + 0x4000, 0x1000))
        .child(Node::new("alias").reg(IOMMU + 0x2000, 0x1000))
        .child(Node::new("second-alias").reg(IOMMU + 0x4000, 0x1000));
    assert!(authorities(tree).unwrap().is_empty());
}

#[test]
fn invalid_controller_geometry_never_falls_back_to_device_export() {
    let cases = [
        Node::new("bad").compatible("riscv,iommu"),
        Node::new("bad")
            .compatible("riscv,iommu")
            .property("reg", []),
        Node::new("bad")
            .compatible("riscv,iommu")
            .property("reg", [0; 17]),
        iommu(IOMMU, 0),
        iommu(u64::MAX - 7, 16),
        iommu(u64::MAX - 7, 1),
        iommu(DRAM as u64, 0x1000),
    ];
    for controller in cases {
        assert!(matches!(
            authorities(root().child(controller)),
            Err(PendFail::Denied)
        ));
    }
}

#[test]
fn unsupported_iommu_discovery_fails_closed() {
    for controller in [
        Node::new("pci-controller")
            .compatible("riscv,pci-iommu")
            .reg(IOMMU, 0x1000),
        Node::new("unknown-controller")
            .property("#iommu-cells", 1u32.to_be_bytes())
            .reg(IOMMU, 0x1000),
    ] {
        assert!(matches!(
            authorities(root().child(controller)),
            Err(PendFail::Denied)
        ));
    }
}

#[test]
fn controller_cell_widths_are_validated_before_parsing_registers() {
    for (address, size) in [
        (vec![0; 3], 2u32.to_be_bytes().to_vec()),
        (3u32.to_be_bytes().to_vec(), 2u32.to_be_bytes().to_vec()),
        (0u32.to_be_bytes().to_vec(), 2u32.to_be_bytes().to_vec()),
        (2u32.to_be_bytes().to_vec(), 0u32.to_be_bytes().to_vec()),
        (2u32.to_be_bytes().to_vec(), vec![0; 8]),
    ] {
        let tree = Node::new("")
            .property("#address-cells", address)
            .property("#size-cells", size)
            .child(iommu(IOMMU, 0x1000));
        assert!(matches!(authorities(tree), Err(PendFail::Denied)));
    }
    let register = [(IOMMU as u32).to_be_bytes(), 0x1000u32.to_be_bytes()].concat();
    let tree = Node::new("")
        .property("#address-cells", 1u32.to_be_bytes())
        .property("#size-cells", 1u32.to_be_bytes())
        .child(
            Node::new("controller")
                .compatible("riscv,iommu")
                .property("reg", register),
        );
    assert!(authorities(tree).unwrap().is_empty());
}

#[test]
fn bus_coordinates_must_be_explicitly_identity_mapped() {
    for bus in [
        Node::new("soc").cells(),
        Node::new("soc").cells().property("ranges", [0; 24]),
    ] {
        assert!(matches!(
            authorities(root().child(bus.child(iommu(IOMMU, 0x1000)))),
            Err(PendFail::Denied)
        ));
    }
    // Identical node names under different parents cannot select the first parent.
    let tree = root()
        .child(
            Node::new("first")
                .cells()
                .property("ranges", [])
                .child(iommu(IOMMU, 0x1000)),
        )
        .child(Node::new("second").cells().child(iommu(IOMMU, 0x1000)));
    assert!(matches!(authorities(tree), Err(PendFail::Denied)));
}

#[test]
fn ram_aliases_do_not_expose_kernel_tables_or_future_allocations() {
    assert!(
        authorities(root().child(Node::new("alias").reg(DRAM as u64, 0x1000)))
            .unwrap()
            .is_empty()
    );
    // A PCI aperture crossing into RAM must still reject the whole plan.
    assert!(matches!(
        authorities(root().child(pci(DRAM as u64 - 1, 2))),
        Err(PendFail::Denied)
    ));
}

#[test]
fn no_controller_preserves_ordinary_grants_and_duplicate_names_fail() {
    assert_eq!(
        authorities(root().child(Node::new("uart").reg(UART, 0x100))).unwrap(),
        [(UART as usize, 0x100)]
    );
    let tree = root()
        .child(Node::new("first").reg(UART, 0x100))
        .child(Node::new("second").reg(UART, 0x1000));
    assert!(matches!(authorities(tree), Err(PendFail::Denied)));
}
