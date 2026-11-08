use super::{plan::Plan, tree::Node};
use kernel_api::abi::call::pend::PendFail;

const RAM: usize = 0x8000_0000;
const UART: usize = 0x1000_0000;
const PLIC: usize = 0x0c00_0000;
const IOMMU: usize = 0x0301_0000;

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_be_bytes()).collect()
}
fn root() -> Node {
    Node::new("").cells()
}
fn bus(name: &'static str, parent: usize) -> Node {
    Node::new(name)
        .cells()
        .property("ranges", words(&[0, 0, 0, parent as u32, 0, 0x10000]))
}
fn grants(root: Node) -> Result<Vec<(usize, usize)>, PendFail> {
    let bytes = root.encode();
    let fdt = fdt::Fdt::new(&bytes).unwrap();
    Plan::discover(&fdt, RAM, 0x10000000).map(|plan| {
        plan.authorities()
            .iter()
            .map(|r| (r.base, r.size))
            .collect()
    })
}

#[test]
fn local_zero_and_each_reg_tuple_translate_to_cpu_coordinates() {
    let registers = [0u64, 0x100, 0x2000, 0x200]
        .into_iter()
        .flat_map(u64::to_be_bytes)
        .collect::<Vec<_>>();
    let tree = root().child(bus("soc", UART).child(Node::new("serial").property("reg", registers)));
    assert_eq!(
        grants(tree).unwrap(),
        [(UART, 0x100), (UART + 0x2000, 0x200)]
    );
}

#[test]
fn nested_buses_use_their_own_cell_widths_and_exact_parent_identity() {
    let inner = Node::new("inner")
        .property("#address-cells", words(&[1]))
        .property("#size-cells", words(&[1]))
        .property("ranges", words(&[0, 0, 0x4000, 0x4000]))
        .child(Node::new("device").property("reg", words(&[0x100, 0x80])));
    let tree = root()
        .child(bus("first", UART).child(inner))
        .child(bus("second", 0x20000000).child(Node::new("device").reg(0x100, 0x80)));
    assert_eq!(
        grants(tree).unwrap(),
        [(UART + 0x4100, 0x80), (0x20000100, 0x80)]
    );
}

#[test]
fn empty_ranges_is_identity_but_absent_ranges_never_is() {
    assert_eq!(
        grants(
            root().child(
                Node::new("soc")
                    .cells()
                    .property("ranges", [])
                    .child(Node::new("uart").reg(UART as u64, 0x100))
            )
        )
        .unwrap(),
        [(UART, 0x100)]
    );
    assert!(
        grants(
            root().child(
                Node::new("soc")
                    .cells()
                    .child(Node::new("uart").reg(UART as u64, 0x100))
            )
        )
        .is_err()
    );
}

#[test]
fn incomplete_ambiguous_and_overflowing_translations_reject_the_whole_plan() {
    for ranges in [
        vec![0; 23],
        words(&[0, 0, 0, UART as u32, 0, 0]),
        words(&[
            0,
            0,
            0,
            UART as u32,
            0,
            0x2000,
            0,
            0x1000,
            0,
            0x20000000,
            0,
            0x2000,
        ]),
        words(&[0, 0, u32::MAX, u32::MAX - 15, 0, 32]),
        words(&[u32::MAX, u32::MAX - 15, 0, UART as u32, 0, 32]),
    ] {
        let tree = root()
            .child(Node::new("valid-first").reg(0x30000000, 4096))
            .child(
                Node::new("soc")
                    .cells()
                    .property("ranges", ranges)
                    .child(Node::new("uart").reg(0x1000, 0x100)),
            );
        assert!(matches!(grants(tree), Err(PendFail::Denied)));
    }
    for (base, size) in [(0x10000, 0x100), (0xff00, 0x200)] {
        assert!(
            grants(root().child(bus("soc", UART).child(Node::new("outside").reg(base, size))))
                .is_err()
        );
    }
}

#[test]
fn malformed_reg_cell_counts_and_duplicate_properties_never_create_authority() {
    for reg in [
        vec![],
        vec![0; 15],
        [0u64, 0].into_iter().flat_map(u64::to_be_bytes).collect(),
        [u64::MAX - 15, 32]
            .into_iter()
            .flat_map(u64::to_be_bytes)
            .collect(),
    ] {
        assert!(grants(root().child(Node::new("bad").property("reg", reg))).is_err());
    }
    for count in [vec![0; 3], words(&[3]), words(&[0]), words(&[1, 2])] {
        let tree = Node::new("")
            .property("#address-cells", count)
            .property("#size-cells", words(&[2]))
            .child(Node::new("bad").reg(UART as u64, 0x100));
        assert!(grants(tree).is_err());
    }
    assert!(
        grants(
            root().child(
                Node::new("duplicate")
                    .reg(UART as u64, 0x100)
                    .property("reg", [0; 16])
            )
        )
        .is_err()
    );
}

#[test]
fn disabled_ancestors_and_identifier_buses_are_not_mmio_authority() {
    let tree = root()
        .child(
            Node::new("disabled-bus")
                .cells()
                .property("status", b"disabled\0")
                .child(Node::new("bad").property("reg", [0; 3])),
        )
        .child(
            Node::new("disabled")
                .reg(UART as u64, 4096)
                .property("status", b"disabled\0"),
        )
        .child(
            Node::new("cpus")
                .property("#address-cells", words(&[1]))
                .property("#size-cells", words(&[0]))
                .child(Node::new("cpu@1").property("reg", words(&[1]))),
        )
        .child(
            Node::new("i2c")
                .property("#address-cells", words(&[1]))
                .property("#size-cells", words(&[0]))
                .reg(0x20000000, 4096)
                .child(Node::new("sensor").property("reg", words(&[0x48]))),
        )
        .child(
            Node::new("reserved-memory")
                .cells()
                .property("ranges", [])
                .child(Node::new("firmware").reg(RAM as u64, 4096)),
        );
    assert_eq!(grants(tree).unwrap(), [(0x20000000, 4096)]);
    assert!(
        grants(
            root().child(
                Node::new("bad-status")
                    .reg(UART as u64, 4096)
                    .property("status", b"okay")
            )
        )
        .is_err()
    );
}

#[test]
fn translated_ram_and_control_aliases_are_withheld_without_an_iommu_route() {
    for physical in [RAM, PLIC, IOMMU, 0x02000000] {
        let tree = root()
            .child(bus("alias-bus", physical).child(Node::new("alias").reg(0x100, 0x10)))
            .child(
                Node::new("plic")
                    .compatible("riscv,plic0")
                    .reg(PLIC as u64, 0x10000),
            )
            .child(
                Node::new("timer")
                    .compatible("riscv,clint0")
                    .reg(0x02000000, 0x10000),
            )
            .child(
                Node::new("controller")
                    .compatible("riscv,iommu")
                    .reg(IOMMU as u64, 4096),
            );
        assert!(grants(tree).unwrap().is_empty());
    }
}

#[test]
fn overlapping_physical_pages_cannot_create_independent_roots() {
    for (first, second) in [(UART, UART), (UART, UART + 0x100)] {
        let tree = root()
            .child(bus("a", first).child(Node::new("device").reg(0x1000, 0x10)))
            .child(bus("b", second).child(Node::new("device").reg(0x1000, 0x10)));
        assert!(matches!(grants(tree), Err(PendFail::Denied)));
    }
    // Distinct byte addresses in the same CPU page also conflict without buses.
    assert!(
        grants(
            root()
                .child(Node::new("a").reg(UART as u64, 1))
                .child(Node::new("b").reg(UART as u64 + 1, 1))
        )
        .is_err()
    );
}

#[test]
fn final_page_coverage_must_fit_the_intersection_of_all_bus_windows() {
    let narrow = Node::new("soc")
        .cells()
        .property("ranges", words(&[0, 0, 0, UART as u32 + 0x100, 0, 0x100]))
        .child(Node::new("tiny").reg(0, 1));
    assert!(grants(root().child(narrow)).is_err());
    // Offsets need not be page aligned when the final CPU pages fit every window.
    let outer = Node::new("outer")
        .cells()
        .property("ranges", words(&[0, 0x100, 0, UART as u32, 0, 0x3000]));
    let inner = Node::new("inner")
        .cells()
        .property("ranges", words(&[0, 0, 0, 0x100, 0, 0x1000]));
    assert_eq!(
        grants(root().child(outer.child(inner.child(Node::new("tiny").reg(0x80, 1))))).unwrap(),
        [(UART + 0x80, 1)]
    );
    // The last translation alone covers this page; the narrower ancestor does not.
    let outer = Node::new("outer")
        .cells()
        .property("ranges", words(&[0, 0, 0, UART as u32 + 0x800, 0, 0x3000]));
    assert!(
        grants(
            root().child(
                outer.child(
                    Node::new("inner")
                        .cells()
                        .property("ranges", words(&[0, 0, 0, 0, 0, 0x1000]))
                        .child(Node::new("tiny").reg(1, 1))
                )
            )
        )
        .is_err()
    );
}

#[test]
fn pci_apertures_translate_in_parent_coordinates_but_child_regs_remain_pci() {
    let pci = Node::new("pci")
        .property("device_type", b"pci\0")
        .property("#address-cells", words(&[3]))
        .property("#size-cells", words(&[2]))
        .reg(0, 4096)
        .property(
            "ranges",
            words(&[0x02000000, 0, 0x8000, 0, 0x8000, 0, 0x1000]),
        )
        .child(Node::new("endpoint").property("reg", words(&[0, 0, 0, 0, 0x1000])));
    assert_eq!(
        grants(root().child(bus("soc", 0x30000000).child(pci))).unwrap(),
        [(0x30000000, 4096), (0x30008000, 4096)]
    );
}
