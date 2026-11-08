use kernel_api::{
    abi::{
        call::{
            EnvCall,
            pend::{
                PendCall,
                operations::{Accord, Return},
            },
        },
        operation::RegisterCall,
        register::Decode,
    },
    address::VirtAddr,
    model::{
        identity::{Mark, PendId, TaskId},
        modifiable::Modifiable,
        pend::{
            AccordRequest, AccordTarget, Distributable, Limits, Operations, PendRange, ShareRights,
        },
    },
};

fn request() -> AccordRequest {
    AccordRequest {
        src: PendId::mint(17),
        target: AccordTarget::Caller,
        limits: Limits {
            operations: Operations::Share(ShareRights::OPEN),
            modifiable: Modifiable::No,
            distributable: Distributable::No,
            range: PendRange::pages(2, 3).unwrap(),
        },
        mark: Mark::new(u64::MAX),
    }
}

#[test]
fn accord_snapshot_has_a_fixed_checked_little_endian_layout() {
    let mut value = request();
    for target in [AccordTarget::Caller, AccordTarget::Task(TaskId::new(23))] {
        for range in [PendRange::Inherit, PendRange::pages(2, 3).unwrap()] {
            value.target = target;
            value.limits.range = range;
            let bytes = value.to_bytes();
            assert_eq!(bytes.len(), 88);
            assert_eq!(&bytes[..8], &17u64.to_le_bytes());
            assert_eq!(&bytes[24..32], &u64::MAX.to_le_bytes());
            assert_eq!(AccordRequest::from_bytes(&bytes), Some(value));
            assert_eq!(AccordRequest::from_bytes(&bytes[..87]), None);
            assert_eq!(AccordRequest::from_bytes(&[0; 89]), None);
        }
    }
}

#[test]
fn accord_rejects_noncanonical_targets_permissions_and_ranges() {
    for (word, invalid) in [
        (0, 0),
        (1, 2),
        (1, 1),
        (2, 23),
        (4, 99),
        (5, 1u64 << 32),
        (5, 1u64 << 31),
        (6, 2),
        (6, 1u64 << 32),
        (7, 2),
        (8, 2),
        (8, 0),
        (9, u64::MAX),
        (10, 0),
        (10, u64::MAX),
    ] {
        let mut bytes = request().to_bytes();
        bytes[word * 8..(word + 1) * 8].copy_from_slice(&invalid.to_le_bytes());
        assert_eq!(AccordRequest::from_bytes(&bytes), None, "word {word}");
    }
}

#[test]
fn new_calls_do_not_reinterpret_retired_pend_registers() {
    let accord = Accord {
        request: VirtAddr::new(0x4000),
    };
    assert_eq!(accord.pack(), [0x4000, 0, 0, 0, 0, 0]);
    assert_eq!(
        PendCall::from_wire(Accord::SLOT, &accord.pack()),
        Ok(accord.into())
    );
    let returned = Return {
        token: PendId::mint(17),
    };
    assert_eq!(returned.pack(), [17, 0, 0, 0, 0, 0]);
    assert_eq!(
        PendCall::from_wire(Return::SLOT, &returned.pack()),
        Ok(returned.into())
    );
    for slot in [4, 5, 7, 8, 9, 10, 13] {
        for words in [[0; 6], [1; 6], [usize::MAX; 6]] {
            assert_eq!(
                EnvCall::from_wire((7usize << 32) | slot, &words),
                Err(Decode::BadSlot)
            );
        }
    }
}
