use kernel_api::{
    abi::{
        self,
        call::{
            EnvCall,
            flat::{FlatCall, operations as flat},
            nest::{NestCall, operations as nest},
            port::{PortCall, operations as port},
        },
        operation::RegisterCall,
    },
    address::VirtAddr,
    model::{
        flat::{FlatId, FlatInfo},
        identity::PendId,
        nest::{NestId, NestInfo, NestKind, NestPhase},
        port::{PortId, PortInfo},
    },
};
#[test]
fn runtime_calls_keep_distinct_domains_and_reject_bad_kinds() {
    let license = PendId::mint(7);
    let create = flat::Create {
        license,
        size: 8192,
        align: 4096,
        level: 0,
    };
    assert_eq!(flat::Create::SLOT, abi::PREFIX | (12 << 32));
    assert_eq!(
        EnvCall::from_wire(flat::Create::SLOT, &create.pack()).unwrap(),
        EnvCall::Flat(FlatCall::Create {
            license,
            size: 8192,
            align: 4096,
            level: 0,
        })
    );
    let create = nest::Create {
        license,
        kind: NestKind::Internal,
    };
    assert_eq!(
        EnvCall::from_wire(nest::Create::SLOT, &create.pack()).unwrap(),
        EnvCall::Nest(NestCall::Create {
            license,
            kind: NestKind::Internal
        })
    );
    let mut words = create.pack();
    words[1] = 2;
    assert!(EnvCall::from_wire(nest::Create::SLOT, &words).is_err());
    let inspect = port::Inspect {
        token: license,
        out: VirtAddr::new(4096),
    };
    assert_eq!(
        EnvCall::from_wire(port::Inspect::SLOT, &inspect.pack()).unwrap(),
        EnvCall::Port(PortCall::Inspect {
            token: license,
            out: VirtAddr::new(4096)
        })
    );
    assert!(EnvCall::from_wire(abi::PREFIX | (14 << 32) | 1, &[0; 6]).is_err());
}
#[test]
fn organization_snapshots_reject_unknown_identity_geometry_and_phase() {
    let flat = FlatInfo {
        level: kernel_api::model::cell::CellLevel::BASE,
        id: FlatId::new(7).unwrap(),
        offset: 2,
        count: 3,
    };
    assert_eq!(FlatInfo::from_bytes(&flat.to_bytes()), Some(flat));
    let mut bad = flat.to_bytes();
    bad[..8].fill(0);
    assert!(FlatInfo::from_bytes(&bad).is_none());
    bad = flat.to_bytes();
    bad[16..24].fill(0);
    assert!(FlatInfo::from_bytes(&bad).is_none());
    let nest = NestInfo {
        id: NestId::new(8).unwrap(),
        kind: NestKind::External,
        phase: NestPhase::Closing,
        port: 9,
        fault_sequence: 2,
        fault_cause: 13,
        fault_address: 0x4000,
        fault_requester: 8,
    };
    assert_eq!(NestInfo::from_bytes(&nest.to_bytes()), Some(nest));
    let mut bad = nest.to_bytes();
    bad[16] = 5;
    assert!(NestInfo::from_bytes(&bad).is_none());
    let port = PortInfo {
        id: PortId::new(9).unwrap(),
        controller: 1,
        requester: 8,
        address_bits: 38,
        page_size: 4096,
        window_size: 1 << 21,
    };
    assert_eq!(PortInfo::from_bytes(&port.to_bytes()), Some(port));
    let mut bad = port.to_bytes();
    bad[32] = 1;
    assert!(PortInfo::from_bytes(&bad).is_none());
}
