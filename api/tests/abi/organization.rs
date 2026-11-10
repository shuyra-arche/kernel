use kernel_api::{
    abi::{
        self,
        call::{
            EnvCall,
            flat::{FlatCall, operations as flat},
            nest::{NestCall, operations as nest},
        },
        operation::RegisterCall,
    },
    address::VirtAddr,
    model::{
        flat::{FlatId, FlatInfo},
        identity::PendId,
        nest::{NestId, NestInfo, NestKind, NestPhase},
    },
};
#[test]
fn runtime_calls_keep_distinct_domains_and_reject_retired_bindings() {
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
    let create = nest::Create { license };
    assert_eq!(
        EnvCall::from_wire(nest::Create::SLOT, &create.pack()).unwrap(),
        EnvCall::Nest(NestCall::Create { license })
    );
    let inspect = nest::Inspect {
        token: license,
        out: VirtAddr::new(4096),
    };
    assert_eq!(
        EnvCall::from_wire(nest::Inspect::SLOT, &inspect.pack()).unwrap(),
        EnvCall::Nest(NestCall::Inspect {
            token: license,
            out: VirtAddr::new(4096)
        })
    );
    for number in [
        abi::PREFIX | (14 << 32),
        abi::PREFIX | (14 << 32) | 1,
        abi::PREFIX | (13 << 32) | 2,
        abi::PREFIX | (13 << 32) | 3,
        (5usize << 48) | (13 << 32),
        (5usize << 48) | (14 << 32),
    ] {
        assert!(EnvCall::from_wire(number, &[7, 8, 0, 0, 0, 0]).is_err());
    }
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
        fault_sequence: 2,
        fault_cause: 13,
        fault_address: 0x4000,
        fault_requester: 8,
    };
    assert_eq!(NestInfo::WIDTH, 56);
    assert_eq!(NestInfo::from_bytes(&nest.to_bytes()), Some(nest));
    for (at, value) in [(0, 0), (8, 2), (16, 5)] {
        let mut bad = nest.to_bytes();
        bad[at] = value;
        assert!(NestInfo::from_bytes(&bad).is_none());
    }
    for len in 0..NestInfo::WIDTH {
        assert!(NestInfo::from_bytes(&nest.to_bytes()[..len]).is_none());
    }
}
