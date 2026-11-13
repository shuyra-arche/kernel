use kernel_api::{
    abi::{
        self,
        call::{EnvCall, room::operations::SleepUntil},
        operation::RegisterCall,
    },
    boot::{
        capsule::Capsule,
        ledger::{self, Entries},
    },
    model::pend::*,
};

#[test]
fn native_calls_reject_v10_and_removed_adapters() {
    assert_eq!(abi::VERSION, 11);
    let call = SleepUntil { at: 123 };
    assert!(EnvCall::from_wire(SleepUntil::SLOT, &call.pack()).is_ok());
    for version in [0, 10, 12, u16::MAX] {
        let slot = (SleepUntil::SLOT & ((1usize << 48) - 1)) | ((version as usize) << 48);
        assert!(EnvCall::from_wire(slot, &call.pack()).is_err());
    }
    for class in [2, 12, 13] {
        for op in 0..32 {
            assert!(EnvCall::from_wire(abi::PREFIX | (class << 32) | op, &[0; 6]).is_err());
        }
    }
    for op in [0, 2, 3, 4] {
        assert!(EnvCall::from_wire(abi::PREFIX | (10 << 32) | op, &[0; 6]).is_err());
    }
}

#[test]
fn native_permission_decoder_knows_only_selected_rights() {
    for rights in [FlatRights::OCCUPY, FlatRights::VACATE, FlatRights::RESHAPE] {
        assert_eq!(
            Operations::of([3, rights.bits() as usize]),
            Some(Operations::Flat(rights))
        );
    }
    assert_eq!(
        Operations::of([4, NestRights::FAULT.bits() as usize]),
        Some(Operations::Nest(NestRights::FAULT))
    );
    assert!(NestRights::reservation().contains(NestRights::FAULT));
    assert_eq!(ShareRights::association(), ShareRights::ASSOCIATE);
    for bits in [1, 2, 3, ShareRights::ASSOCIATE.bits() | 1] {
        assert!(Operations::of([2, bits as usize]).is_none());
    }
    for kind in 0..7 {
        assert!(Operations::of([kind, usize::MAX]).is_none());
    }
}

#[test]
fn image_and_ledger_must_both_match_native_version() {
    assert_eq!(ledger::VERSION, 11);
    let mut bytes = [0u8; 8];
    ledger::store_entries(&mut bytes, &[]).unwrap();
    assert!(Entries::new(&bytes).is_some());
    for version in [0u32, 10, 12, u32::MAX] {
        bytes[..4].copy_from_slice(&version.to_le_bytes());
        assert!(Entries::new(&bytes).is_none());
    }
    let mut capsule = vec![0u8; 8192];
    capsule[..8].copy_from_slice(b"SQBOOT01");
    capsule[8..10].copy_from_slice(&2u16.to_le_bytes());
    capsule[10] = 12;
    capsule[11] = 11;
    capsule[12..16].copy_from_slice(&1u32.to_le_bytes());
    for (at, v) in [
        (16, 0x1000u64),
        (24, 8192),
        (32, 0x1000),
        (40, 1),
        (48, 1),
        (56, 4096),
        (64, 5),
    ] {
        capsule[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }
    assert!(Capsule::parse(&capsule).is_some());
    for version in [0, 10, 12, 255] {
        capsule[11] = version;
        assert!(Capsule::parse(&capsule).is_none());
    }
}
