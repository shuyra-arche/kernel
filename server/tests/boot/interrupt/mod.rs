use crate::{interrupt_plan::Plan, tree::Node};
fn words(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}
fn tree(interrupts: &[u32], contexts: &[u32], size: u64) -> Node {
    Node::new("")
        .cells()
        .child(
            Node::new("cpus")
                .property("#address-cells", words(&[1]))
                .property("#size-cells", words(&[0]))
                .child(
                    Node::new("cpu@0").property("reg", words(&[0])).child(
                        Node::new("interrupt-controller")
                            .property("phandle", words(&[2]))
                            .property("#interrupt-cells", words(&[1])),
                    ),
                ),
        )
        .child(
            Node::new("plic")
                .compatible("riscv,plic0")
                .property("phandle", words(&[1]))
                .property("#interrupt-cells", words(&[1]))
                .property("riscv,ndev", words(&[95]))
                .property("interrupts-extended", words(contexts))
                .reg(0xc000000, size),
        )
        .child(
            Node::new("iommu")
                .compatible("riscv,iommu")
                .property("interrupt-parent", words(&[1]))
                .property("interrupts", words(interrupts))
                .reg(0x3010000, 4096),
        )
}
#[test]
fn routes_only_supervisor_context_and_cq_fq_pq() {
    for words in [vec![36, 37, 38, 39], vec![36, 1, 37, 1, 38, 1, 39, 1]] {
        let bytes = tree(&words, &[2, 11, 2, 9], 0x600000).encode();
        let fdt = fdt::Fdt::new(&bytes).unwrap();
        let plan = Plan::discover(&fdt, 0, 0x3010000).unwrap();
        assert_eq!(plan.contexts, [1]);
        assert_eq!(plan.selected, 1);
        assert_eq!(plan.queues, [36, 37, 39]);
        assert!(Plan::discover(&fdt, 1, 0x3010000).is_err());
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
        assert!(Plan::discover(&fdt::Fdt::new(&bytes).unwrap(), 0, 0x3010000).is_err());
    }
}
