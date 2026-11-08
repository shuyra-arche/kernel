use super::{
    hart_facts::{HartError, HartInfo, Paging},
    hart_id::{FirmwareId, HartId},
    tree::Node,
};

fn cpu(id: u64, mode: &str) -> Node {
    bare(id)
        .property("mmu-type", format!("{mode}\0"))
        .property("riscv,isa", b"rv64imafdc\0")
}
fn bare(id: u64) -> Node {
    Node::new("cpu@0")
        .property("device_type", b"cpu\0")
        .property("reg", id.to_be_bytes())
}
fn tree(nodes: Vec<Node>) -> Vec<u8> {
    let mut cpus = Node::new("cpus")
        .property("#address-cells", 2u32.to_be_bytes())
        .property("#size-cells", 0u32.to_be_bytes())
        .property("timebase-frequency", 10_000_000u32.to_be_bytes());
    for node in nodes {
        cpus = cpus.child(node);
    }
    Node::new("").cells().child(cpus).encode()
}
fn parse(nodes: Vec<Node>, boot: usize) -> Result<HartInfo, HartError> {
    HartInfo::parse(&fdt::Fdt::new(&tree(nodes)).unwrap(), FirmwareId::new(boot))
}

#[test]
fn sparse_hardware_ids_get_dense_slots_with_boot_first() {
    let harts = parse(
        vec![
            cpu(4097, "riscv,sv57"),
            cpu(7, "riscv,sv48"),
            cpu(129, "riscv,sv39"),
        ],
        7,
    )
    .unwrap();
    assert_eq!(harts.count, 3);
    assert_eq!(harts.hertz, 10_000_000);
    for (slot, physical) in [7, 4097, 129].into_iter().enumerate() {
        assert_eq!(harts.firmware(HartId::new(slot)).get(), physical);
        // SBI base is the hardware number, including values above mask width.
        assert_eq!(harts.firmware(HartId::new(slot)).target(), (1, physical));
    }
    assert_eq!(harts.common.paging, Paging::Sv39);
}

#[test]
fn disabled_cpus_do_not_constrain_capabilities_or_receive_slots() {
    let harts = parse(
        vec![
            bare(1).property("status", b"disabled\0"),
            cpu(70, "riscv,sv57"),
        ],
        70,
    )
    .unwrap();
    assert_eq!(harts.count, 1);
    assert_eq!(harts.common.paging, Paging::Sv57);
    assert!(matches!(
        parse(
            vec![cpu(1, "riscv,sv39").property("status", b"disabled\0")],
            1
        ),
        Err(HartError::Boot(1))
    ));
    assert!(matches!(
        parse(vec![cpu(1, "riscv,sv39")], 2),
        Err(HartError::Boot(2))
    ));
}

#[test]
fn rejects_duplicate_and_sbi_reserved_ids_including_disabled_aliases() {
    assert!(matches!(
        parse(
            vec![
                cpu(1, "riscv,sv39"),
                bare(1).property("status", b"disabled\0")
            ],
            1
        ),
        Err(HartError::Identity(1))
    ));
    assert!(matches!(
        parse(vec![cpu(u64::MAX, "riscv,sv39")], usize::MAX),
        Err(HartError::Identity(_))
    ));
}

#[test]
fn paging_requires_explicit_compatible_description_for_every_enabled_cpu() {
    for mode in ["riscv,none", "riscv,sv32", "sv39", "riscv,sv64"] {
        assert!(parse(vec![cpu(0, "riscv,sv57"), cpu(1, mode)], 0).is_err());
    }
    assert!(parse(vec![bare(0).property("riscv,isa", b"rv64imafdc\0")], 0).is_err());
    let harts = parse(vec![cpu(0, "riscv,sv57"), cpu(1, "riscv,sv48")], 1).unwrap();
    assert_eq!(harts.common.paging, Paging::Sv48);
}

fn modern(id: u64, list: &[u8]) -> Node {
    bare(id)
        .property("mmu-type", b"riscv,sv39\0")
        .property("riscv,isa-base", b"rv64i\0")
        .property("riscv,isa-extensions", list)
}
const EXTENSIONS: &[u8] = b"i\0m\0a\0f\0d\0c\0zicsr\0zifencei\0zicntr\0";

#[test]
fn new_isa_pair_is_authoritative_and_legacy_i_has_frozen_binding_semantics() {
    let harts = parse(
        vec![
            modern(1, EXTENSIONS).property("riscv,isa", b"rv32i\0"),
            cpu(3, "riscv,sv48"),
        ],
        1,
    )
    .unwrap();
    assert_eq!(harts.count, 2);
    for omitted in ["i", "m", "a", "f", "d", "c", "zicsr", "zifencei", "zicntr"] {
        let list = EXTENSIONS
            .split(|b| *b == 0)
            .filter(|v| !v.is_empty() && *v != omitted.as_bytes())
            .flat_map(|v| v.iter().copied().chain([0]))
            .collect::<Vec<_>>();
        assert!(
            matches!(parse(vec![modern(1, &list)], 1), Err(HartError::Isa(1))),
            "{omitted}"
        );
    }
    let incomplete = bare(1)
        .property("mmu-type", b"riscv,sv39\0")
        .property("riscv,isa-base", b"rv64i\0")
        .property("riscv,isa", b"rv64imafdc\0");
    assert!(parse(vec![incomplete], 1).is_err());
}

#[test]
fn malformed_properties_fail_before_any_hart_is_admitted() {
    for node in [
        cpu(0, "riscv,sv39").property("status", b"okay"),
        cpu(0, "riscv,sv39").property("status", b"okay\0junk\0"),
        cpu(0, "riscv,sv39").property("reg", 0u64.to_be_bytes()),
        cpu(0, "riscv,sv39").property("mmu-type", b"riscv,sv48\0"),
        modern(0, b"i\0i\0"),
        modern(0, b"i\0\0"),
        modern(0, b"i\0m"),
        bare(0)
            .property("mmu-type", b"riscv,sv39\0")
            .property("riscv,isa", b"rv64imafdc!\0"),
    ] {
        assert!(parse(vec![node], 0).is_err());
    }
}

#[test]
fn cpu_cell_widths_and_clock_are_validated() {
    let encode = |width, size, frequency, reg: Vec<u8>| {
        Node::new("")
            .cells()
            .child(
                Node::new("cpus")
                    .property("#address-cells", u32::to_be_bytes(width))
                    .property("#size-cells", u32::to_be_bytes(size))
                    .property("timebase-frequency", u32::to_be_bytes(frequency))
                    .child(
                        Node::new("cpu@7")
                            .property("device_type", b"cpu\0")
                            .property("reg", reg)
                            .property("mmu-type", b"riscv,sv39\0")
                            .property("riscv,isa", b"rv64imafdc\0"),
                    ),
            )
            .encode()
    };
    for (width, size, frequency, reg, valid) in [
        (1, 0, 1, 7u32.to_be_bytes().to_vec(), true),
        (1, 0, 1, 7u64.to_be_bytes().to_vec(), false),
        (2, 0, 1, 7u32.to_be_bytes().to_vec(), false),
        (3, 0, 1, 7u64.to_be_bytes().to_vec(), false),
        (2, 1, 1, 7u64.to_be_bytes().to_vec(), false),
        (2, 0, 0, 7u64.to_be_bytes().to_vec(), false),
    ] {
        let bytes = encode(width, size, frequency, reg);
        assert_eq!(
            HartInfo::parse(&fdt::Fdt::new(&bytes).unwrap(), FirmwareId::new(7)).is_ok(),
            valid
        );
    }
}

#[test]
fn early_capacity_is_bounded_and_not_a_limit_on_hardware_id_values() {
    let nodes = (0..256).map(|i| cpu(10000 + i, "riscv,sv39")).collect();
    assert_eq!(parse(nodes, 10255).unwrap().count, 256);
    let nodes = (0..257).map(|i| cpu(i, "riscv,sv39")).collect();
    assert!(matches!(parse(nodes, 0), Err(HartError::Capacity)));
}
