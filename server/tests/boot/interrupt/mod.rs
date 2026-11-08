use crate::hart_facts::HartInfo;
use crate::hart_id::{FirmwareId, HartId};
use crate::interrupt_plan::Plan;
use crate::tree::Node;
fn words(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}
fn local(key: &'static str, handle: u32) -> Node {
    Node::new("interrupt-controller")
        .compatible("riscv,cpu-intc")
        .property("interrupt-controller", [])
        .property(key, words(&[handle]))
        .property("#interrupt-cells", words(&[1]))
}
fn cpu(id: u32, local: Node) -> Node {
    Node::new("cpu@0")
        .property("reg", words(&[id]))
        .property("device_type", b"cpu\0")
        .property("mmu-type", b"riscv,sv39\0")
        .property("riscv,isa", b"rv64imafdc\0")
        .child(local)
}
fn cpus() -> Node {
    Node::new("cpus")
        .property("#address-cells", words(&[1]))
        .property("#size-cells", words(&[0]))
        .property("timebase-frequency", words(&[10_000_000]))
}
fn plic(key: &'static str, contexts: &[u32], size: u64) -> Node {
    Node::new("plic")
        .compatible("riscv,plic0")
        .property(key, words(&[1]))
        .property("#interrupt-cells", words(&[1]))
        .property("riscv,ndev", words(&[95]))
        .property("interrupts-extended", words(contexts))
        .reg(0xc000000, size)
}
fn iommu(interrupts: &[u32]) -> Node {
    Node::new("iommu")
        .compatible("riscv,iommu")
        .property("phandle", words(&[3]))
        .property("#iommu-cells", words(&[1]))
        .property("interrupt-parent", words(&[1]))
        .property("interrupts", words(interrupts))
        .reg(0x3010000, 4096)
}
fn root() -> Node {
    Node::new("").cells()
}
fn tree(interrupts: &[u32], contexts: &[u32], size: u64) -> Node {
    root()
        .child(cpus().child(cpu(0, local("phandle", 2))))
        .child(plic("phandle", contexts, size))
        .child(iommu(interrupts))
}
#[test]
fn routes_only_supervisor_context_and_cq_fq_pq() {
    for words in [vec![36, 37, 38, 39], vec![36, 1, 37, 1, 38, 1, 39, 1]] {
        let bytes = tree(&words, &[2, 11, 2, 9], 0x600000).encode();
        let fdt = fdt::Fdt::new(&bytes).unwrap();
        let harts = HartInfo::parse(&fdt, FirmwareId::new(0)).unwrap();
        let plan = Plan::discover(&fdt, &harts, HartId::new(0), 0x3010000).unwrap();
        assert_eq!(plan.contexts, [1]);
        assert_eq!(plan.selected, 1);
        assert_eq!(plan.queues, [36, 37, 39]);
        assert!(Plan::discover(&fdt, &harts, HartId::new(1), 0x3010000).is_err());
    }
}
#[test]
fn ambiguous_or_unbounded_interrupt_routes_are_rejected() {
    for (irqs, contexts, size) in [
        (vec![36, 36, 38, 39], vec![2, 11, 2, 9], 0x600000),
        (vec![36, 37, 38, 96], vec![2, 11, 2, 9], 0x600000),
        (
            vec![36, 1, 37, 0, 38, 1, 39, 1],
            vec![2, 11, 2, 9],
            0x600000,
        ),
        (vec![36, 37, 38, 39], vec![2, 9, 2, 9], 0x600000),
        (vec![36, 37, 38, 39], vec![7, 11, 7, 9], 0x600000),
        (vec![36, 37, 38, 39], vec![2, 11, 2, 9], 0x201000),
    ] {
        let bytes = tree(&irqs, &contexts, size).encode();
        assert!(discover(&bytes).is_err());
    }
}

fn discover(bytes: &[u8]) -> Result<Plan, ()> {
    let fdt = fdt::Fdt::new(bytes).map_err(|_| ())?;
    let harts = HartInfo::parse(&fdt, FirmwareId::new(0)).map_err(|_| ())?;
    Plan::discover(&fdt, &harts, HartId::new(0), 0x3010000).map_err(|_| ())
}

#[test]
fn legacy_phandle_aliases_share_one_tree_wide_identity_rule() {
    let bytes = root()
        .child(cpus().child(cpu(0, local("linux,phandle", 2))))
        .child(plic("linux,phandle", &[2, 11, 2, 9], 0x600000))
        .child(iommu(&[36, 37, 38, 39]).property("linux,phandle", words(&[3])))
        .encode();
    assert_eq!(discover(&bytes).unwrap().selected, 1);
    let fdt = fdt::Fdt::new(&bytes).unwrap();
    assert_eq!(
        crate::topology::Topology::discover(&fdt)
            .unwrap()
            .controllers()
            .collect::<Vec<_>>(),
        [0x3010000]
    );
}

#[test]
fn global_identity_ambiguity_is_rejected_by_both_route_consumers() {
    for extra in [
        Node::new("unrelated").property("phandle", words(&[2])),
        Node::new("unrelated")
            .property("phandle", words(&[4]))
            .property("phandle", words(&[4])),
        Node::new("unrelated")
            .property("phandle", words(&[4]))
            .property("linux,phandle", words(&[5])),
        Node::new("unrelated").property("linux,phandle", words(&[0])),
        Node::new("unrelated").property("phandle", words(&[u32::MAX])),
    ] {
        let bytes = tree(&[36, 37, 38, 39], &[2, 11, 2, 9], 0x600000)
            .child(extra)
            .encode();
        assert!(discover(&bytes).is_err());
        assert!(crate::topology::Topology::discover(&fdt::Fdt::new(&bytes).unwrap()).is_err());
    }
}

#[test]
fn plic_uses_admitted_membership_with_sparse_or_disabled_cpu_nodes() {
    let bytes = root()
        .child(
            cpus()
                .child(cpu(0, local("phandle", 2)).property("status", b"disabled\0"))
                .child(cpu(4097, local("phandle", 4))),
        )
        .child(plic("phandle", &[2, 11, 2, 9, 4, 11, 4, 9], 0x600000))
        .child(iommu(&[36, 37, 38, 39]))
        .encode();
    let fdt = fdt::Fdt::new(&bytes).unwrap();
    let harts = HartInfo::parse(&fdt, FirmwareId::new(4097)).unwrap();
    let plan = Plan::discover(&fdt, &harts, HartId::new(0), 0x3010000).unwrap();
    // Disabled supervisor contexts are masked, but cannot become the selected route.
    assert_eq!(plan.contexts, [1, 3]);
    assert_eq!(plan.selected, 3);
    // Equal node text in a different DTB is not the admitted CPU identity.
    let copy = bytes.clone();
    assert!(
        Plan::discover(
            &fdt::Fdt::new(&copy).unwrap(),
            &harts,
            HartId::new(0),
            0x3010000
        )
        .is_err()
    );
}

#[test]
fn local_controller_shape_and_status_cannot_select_a_false_route() {
    for local in [
        local("phandle", 2).property("#interrupt-cells", words(&[1])),
        local("phandle", 2).property("status", b"disabled\0"),
        local("phandle", 2).property("status", b"okay\0extra\0"),
        Node::new("not-an-intc")
            .property("phandle", words(&[2]))
            .property("#interrupt-cells", words(&[1])),
        Node::new("interrupt-controller")
            .compatible("riscv,cpu-intc")
            .property("phandle", words(&[2]))
            .property("#interrupt-cells", words(&[1]))
            .property("interrupt-controller", words(&[1])),
    ] {
        let bytes = root()
            .child(cpus().child(cpu(0, local)))
            .child(plic("phandle", &[2, 11, 2, 9], 0x600000))
            .child(iommu(&[36, 37, 38, 39]))
            .encode();
        assert!(discover(&bytes).is_err());
    }
}

#[test]
fn interrupt_registers_follow_shared_geometry_and_identity_bus_constraints() {
    for plic in [
        plic("phandle", &[2, 11, 2, 9], 0x600000).reg(0xd000000, 0x600000),
        plic("phandle", &[2, 11, 2, 9], 0x600000).property("interrupts-extended", words(&[2, 9])),
        Node::new("bus")
            .cells()
            .child(plic("phandle", &[2, 11, 2, 9], 0x600000)),
        Node::new("bus")
            .cells()
            .property(
                "ranges",
                [0xc000000u64, 0xd000000, 0x600000]
                    .into_iter()
                    .flat_map(u64::to_be_bytes)
                    .collect::<Vec<_>>(),
            )
            .child(plic("phandle", &[2, 11, 2, 9], 0x600000)),
        Node::new("bus")
            .cells()
            .property("ranges", [])
            .property("status", b"disabled\0")
            .child(plic("phandle", &[2, 11, 2, 9], 0x600000)),
    ] {
        let bytes = root()
            .child(cpus().child(cpu(0, local("phandle", 2))))
            .child(plic)
            .child(iommu(&[36, 37, 38, 39]))
            .encode();
        assert!(discover(&bytes).is_err());
    }
}

#[test]
fn selected_iommu_must_have_unique_valid_register_and_irq_properties() {
    for controller in [
        iommu(&[36, 37, 38, 39]).reg(0x4010000, 4096),
        iommu(&[36, 37, 38, 39]).property("interrupt-parent", words(&[1])),
        iommu(&[36, 37, 38, 39]).property("interrupts", words(&[36, 37, 38, 39])),
        iommu(&[36, 37, 38, 39]).property("status", b"disabled\0"),
    ] {
        let bytes = root()
            .child(cpus().child(cpu(0, local("phandle", 2))))
            .child(plic("phandle", &[2, 11, 2, 9], 0x600000))
            .child(controller)
            .encode();
        assert!(discover(&bytes).is_err());
    }
}
