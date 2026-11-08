use super::{
    Region,
    memory_facts::{Memory, MemoryError},
    tree::Node,
};

const RAM: usize = 0x8000_0000;
const DTB: usize = RAM + 0x8000;
const KERNEL: Region = Region::new(RAM + 0x2000, 0x3000);

fn root() -> Node {
    Node::new("")
        .cells()
        .child(Node::new("memory@80000000").reg(RAM as u64, 0x10000))
}
fn reserved() -> Node {
    Node::new("reserved-memory").cells().property("ranges", [])
}
fn chosen(start: usize, end: usize) -> Node {
    Node::new("chosen")
        .property("linux,initrd-start", (start as u64).to_be_bytes())
        .property("linux,initrd-end", (end as u64).to_be_bytes())
}
fn parse(node: Node) -> Result<Memory, MemoryError> {
    Memory::parse(&node.encode(), DTB, KERNEL)
}
fn pairs(memory: &Memory) -> Vec<(usize, usize)> {
    memory
        .free()
        .iter()
        .map(|r| (r.base - RAM, r.size))
        .collect()
}

#[test]
fn all_banks_and_reservations_constrain_both_heaps() {
    let bytes = root()
        .child(Node::new("memory@80020000").reg((RAM + 0x20000) as u64, 0x10000))
        .child(chosen(RAM + 0xc000, RAM + 0xd001))
        .child(
            reserved()
                .child(Node::new("firmware").reg((RAM + 0x1ff) as u64, 0x100))
                .child(Node::new("overlap").reg((RAM + 0x100) as u64, 0x200))
                .child(Node::new("tail").reg((RAM + 0xff00) as u64, 0x200)),
        )
        .encode_reserved(&[
            ((RAM + 0x6001) as u64, 0x100),
            ((RAM + 0x21000) as u64, 0x1000),
        ]);
    let m = Memory::parse(&bytes, DTB, KERNEL).unwrap();
    assert_eq!(
        pairs(&m),
        vec![
            (0x1000, 0x1000),
            (0x5000, 0x1000),
            (0x7000, 0x1000),
            (0x9000, 0x3000),
            (0xe000, 0x1000),
            (0x20000, 0x1000),
            (0x22000, 0xe000)
        ]
    );
    assert_eq!(m.bootstrap(), Region::new(RAM + 0x22000, 0xe000));
    assert!(!m.contains_identity(Region::new(RAM + 0x10000, 4096)));
    assert_eq!(m.initrd(), Some(Region::new(RAM + 0xc000, 0x2000)));
    assert_eq!(m.dtb(), Region::new(DTB, 4096));
}

#[test]
fn every_reg_tuple_is_used_and_disabled_banks_are_ignored() {
    let m = parse(
        Node::new("")
            .cells()
            .child(
                Node::new("memory@80000000").property(
                    "reg",
                    [RAM as u64, 0x10000, (RAM + 0x30000) as u64, 0x10000]
                        .into_iter()
                        .flat_map(u64::to_be_bytes)
                        .collect::<Vec<_>>(),
                ),
            )
            .child(
                Node::new("memory@80010000")
                    .property("status", b"disabled\0")
                    .reg((RAM + 0x10000) as u64, 0x10000),
            ),
    )
    .unwrap();
    assert_eq!(
        m.free().last().unwrap(),
        &Region::new(RAM + 0x30000, 0x10000)
    );
    assert!(!m.contains_identity(Region::new(RAM + 0x10000, 4096)));
}

#[test]
fn no_map_pages_are_absent_from_identity_mapping() {
    let m = parse(
        root().child(
            reserved().child(
                Node::new("secret")
                    .reg((RAM + 0xa001) as u64, 1)
                    .property("no-map", []),
            ),
        ),
    )
    .unwrap();
    assert!(
        m.identity()
            .iter()
            .all(|r| !(r.base <= RAM + 0xa000 && RAM + 0xa000 < r.range().end))
    );
    assert!(
        m.free()
            .iter()
            .all(|r| !(r.base <= RAM + 0xa000 && RAM + 0xa000 < r.range().end))
    );
}

#[test]
fn overlapping_ram_and_boot_objects_fail_before_allocation() {
    assert_eq!(
        parse(root().child(Node::new("memory@80008000").reg(DTB as u64, 0x10000))).unwrap_err(),
        MemoryError::Overlap
    );
    assert_eq!(
        parse(root().child(chosen(DTB, DTB + 4096))).unwrap_err(),
        MemoryError::Overlap
    );
    assert_eq!(
        parse(root().child(reserved().child(Node::new("firmware").reg(KERNEL.base as u64, 4096))))
            .unwrap_err(),
        MemoryError::Overlap
    );
    assert_eq!(
        parse(root().child(chosen(KERNEL.base, KERNEL.base + 4096))).unwrap_err(),
        MemoryError::Overlap
    );
}

#[test]
fn invalid_memory_facts_are_not_silently_truncated() {
    for node in [
        Node::new("memory@bad").reg(u64::MAX - 4095, 8192),
        Node::new("memory@bad").reg(0, 0),
        Node::new("memory@bad").property("reg", [0; 15]),
        Node::new("memory@bad").reg((RAM + 0x20001) as u64, 4096),
        Node::new("memory@bad")
            .reg((RAM + 0x20000) as u64, 4096)
            .property("status", b"okay"),
    ] {
        assert!(parse(root().child(node)).is_err());
    }
    assert!(parse(root().child(chosen(RAM + 0xf000, RAM + 0x11000))).is_err());
    assert!(parse(root().child(chosen(RAM + 1, RAM + 4096))).is_err());
    assert!(
        parse(
            root().child(
                reserved().child(Node::new("dynamic").property("size", 4096u64.to_be_bytes()))
            )
        )
        .is_err()
    );
    assert!(
        parse(
            root().child(
                Node::new("reserved-memory")
                    .cells()
                    .child(Node::new("unknown").reg(0, 4096))
            )
        )
        .is_err()
    );
}

#[test]
fn fixed_capacity_exhaustion_and_no_free_pages_are_explicit() {
    let mut reserved_node = reserved();
    for i in 0..129 {
        reserved_node =
            reserved_node.child(Node::new("reservation").reg(0x100000 + (i * 0x2000), 4096));
    }
    assert_eq!(
        parse(root().child(reserved_node)).unwrap_err(),
        MemoryError::Capacity
    );
    let bytes = Node::new("")
        .cells()
        .child(Node::new("memory").reg(RAM as u64, 8192))
        .encode();
    assert_eq!(
        Memory::parse(&bytes, RAM + 4096, Region::new(RAM, 4096)).unwrap_err(),
        MemoryError::Empty
    );
}

#[test]
fn malformed_fdt_envelopes_return_errors_without_panics() {
    let bytes = root().encode();
    for end in 0..bytes.len() {
        assert!(Memory::parse(&bytes[..end], DTB, KERNEL).is_err());
    }
    for (at, value) in [
        (8, u32::MAX),
        (12, u32::MAX),
        (16, 41),
        (32, u32::MAX),
        (36, u32::MAX),
        (56, 3),
    ] {
        let mut bad = bytes.clone();
        bad[at..at + 4].copy_from_slice(&value.to_be_bytes());
        assert_eq!(
            Memory::parse(&bad, DTB, KERNEL).unwrap_err(),
            MemoryError::Tree
        );
    }
    // Corrupt each encoded word independently, including token lengths and
    // string offsets. A malformed fixture may be rejected, never panic inside
    // the third-party FDT iterators before boot can report the failure.
    for at in (0..bytes.len().saturating_sub(3)).step_by(4) {
        let mut bad = bytes.clone();
        bad[at..at + 4].fill(0xff);
        assert!(std::panic::catch_unwind(|| Memory::parse(&bad, DTB, KERNEL)).is_ok());
    }
}
