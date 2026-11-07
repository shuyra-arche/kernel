mod fixtures;
use crate::{plan::Plan, topology::RouteError, tree::Node};
use fixtures::*;

#[test]
fn routes_use_half_open_intervals_and_controller_translation_without_bypass() {
    let topology = discover(
        root()
            .child(controller(CONTROLLER, 1))
            .child(controller(CONTROLLER + 4096, 2))
            .child(host(HOST, &[0, 1, 0, 0, 8, 1, 16, 8, 0x100, 2, 0, 0x100])),
    )
    .unwrap();
    for (rid, base, did) in [
        (8, CONTROLLER, 16),
        (15, CONTROLLER, 23),
        (0x100, CONTROLLER + 4096, 0),
        (0x1ff, CONTROLLER + 4096, 255),
    ] {
        let route = topology.resolve(HOST, rid).unwrap();
        assert_eq!((route.controller, route.device), (base, did));
    }
    for rid in [0, 7, 16, 0xff, 0x200, 0xffff] {
        assert!(matches!(
            topology.resolve(HOST, rid),
            Err(RouteError::Unmapped)
        ));
    }
    assert!(matches!(
        topology.resolve(HOST + 4096, 8),
        Err(RouteError::Unmapped)
    ));
}

#[test]
fn ambiguous_identity_masks_input_ranges_and_output_aliases_are_rejected() {
    let cases = [
        root()
            .child(controller(CONTROLLER, 1))
            .child(Node::new("unrelated").property("phandle", 1u32.to_be_bytes())),
        root()
            .child(controller(CONTROLLER, 1))
            .child(controller(CONTROLLER, 2)),
        root()
            .child(controller(CONTROLLER, 1))
            .child(host(HOST, &[0, 1, 0, 64]).property("iommu-map-mask", 0xfff8u32.to_be_bytes())),
        root()
            .child(controller(CONTROLLER, 1))
            .child(host(HOST, &[0, 1, 0, 16, 8, 1, 32, 16])),
        root()
            .child(controller(CONTROLLER, 1))
            .child(host(HOST, &[0, 1, 0, 16, 32, 1, 8, 16])),
        root()
            .child(controller(CONTROLLER, 1))
            .child(host(HOST, &[0, 1, 0, 16]))
            .child(host(0x50000000, &[0, 1, 8, 16])),
        root()
            .child(controller(CONTROLLER, 1))
            .child(host(HOST, &[0, 1, 0, 16]))
            .child(host(HOST + 0x100000, &[0, 1, 32, 16])),
    ];
    for tree in cases {
        assert!(discover(tree).is_err());
    }
}

#[test]
fn malformed_unknown_disabled_and_translated_routes_never_become_authority() {
    for map in [
        vec![],
        vec![0, 1, 0],
        vec![0, 2, 0, 8],
        vec![0xffff, 1, 0, 2],
        vec![0, 1, 0xffffff, 2],
        vec![u32::MAX, 1, 0, 1],
    ] {
        assert!(
            discover(
                root()
                    .child(controller(CONTROLLER, 1))
                    .child(host(HOST, &map))
            )
            .is_err()
        );
    }
    for status in ["disabled\0", "reserved\0", "fail\0"] {
        assert!(
            discover(
                root()
                    .child(controller(CONTROLLER, 1).property("status", status))
                    .child(host(HOST, &[0, 1, 0, 16]))
            )
            .is_err()
        );
    }
    let bus = Node::new("bus").cells().child(controller(CONTROLLER, 1));
    assert!(discover(root().child(bus).child(host(HOST, &[0, 1, 0, 16]))).is_err());
    let disabled_bus = Node::new("bus")
        .cells()
        .property("ranges", [])
        .property("status", "disabled\0")
        .child(controller(CONTROLLER, 1));
    assert!(discover(root().child(disabled_bus).child(host(HOST, &[0, 1, 0, 16]))).is_err());
    assert!(
        discover(
            root()
                .child(controller(CONTROLLER, 1))
                .child(host(HOST, &[0, 1, 0, 16]).property("iommus", words(&[1, 8])))
        )
        .is_err()
    );
}

#[test]
fn protected_configuration_and_aliases_are_withheld_before_boot_grants() {
    let root = root()
        .child(controller(CONTROLLER, 1))
        .child(host(HOST, &[0, 1, 0, 65536]))
        .child(Node::new("config-alias").reg(HOST as u64 + 0x8000, 4096))
        .child(Node::new("device-bar").reg(0x40000000, 0x100000));
    let bytes = root.encode();
    let fdt = fdt::Fdt::new(&bytes).unwrap();
    let plan = Plan::discover(&fdt, 0x80000000, 0x10000000).unwrap();
    let grants: Vec<_> = plan.grants().iter().map(|r| (r.base, r.size)).collect();
    assert_eq!(grants, [(0x40000000, 0x100000)]);
}

#[test]
fn disabled_hosts_have_no_route_and_still_cannot_export_configuration() {
    let root = root()
        .child(controller(CONTROLLER, 1))
        .child(host(HOST, &[0, 1, 0, 65536]).property("status", "disabled\0"));
    let bytes = root.encode();
    let fdt = fdt::Fdt::new(&bytes).unwrap();
    let topology = crate::topology::Topology::discover(&fdt).unwrap();
    assert!(matches!(
        topology.resolve(HOST, 8),
        Err(RouteError::Unmapped)
    ));
    assert_eq!(
        topology.configurations().collect::<Vec<_>>(),
        [(HOST, 0x10000000)]
    );
    assert!(
        Plan::discover(&fdt, 0x80000000, 0x10000000)
            .unwrap()
            .grants()
            .is_empty()
    );
}

#[test]
fn protected_bar_windows_and_plic_aliases_never_reach_units() {
    let host = host(HOST, &[0, 1, 0, 65536])
        .property("#address-cells", words(&[3]))
        .property("#size-cells", words(&[2]))
        .property(
            "ranges",
            words(&[0x02000000, 0, 0x40000000, 0, 0x40000000, 0, 0x10000000]),
        );
    let bytes = root()
        .child(controller(CONTROLLER, 1))
        .child(host)
        .child(
            Node::new("plic")
                .compatible("riscv,plic0")
                .reg(0xc000000, 0x600000),
        )
        .child(Node::new("plic-alias").reg(0xc001000, 4096))
        .child(Node::new("bar-alias").reg(0x40001000, 4096))
        .child(Node::new("uart").reg(0x10000000, 4096))
        .encode();
    let fdt = fdt::Fdt::new(&bytes).unwrap();
    let plan = Plan::discover(&fdt, 0x80000000, 0x10000000).unwrap();
    assert_eq!(
        plan.grants()
            .iter()
            .map(|r| (r.base, r.size))
            .collect::<Vec<_>>(),
        [(0x10000000, 4096)]
    );
}
