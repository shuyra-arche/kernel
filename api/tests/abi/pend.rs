use super::fetch_words;
use kernel_api::{
    abi::{
        call::{
            EnvCall,
            pend::{
                PendCall,
                operations::{Accord, Narrow, Return},
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
            AccordRequest, AccordTarget, Distributable, Limits, NarrowRequest, Operations,
            PendRange, PendScope, ShareRights,
        },
    },
};

fn request() -> AccordRequest {
    AccordRequest {
        src: PendId::mint(17),
        target: AccordTarget::Caller,
        limits: Limits {
            operations: Operations::Share(ShareRights::ASSOCIATE),
            modifiable: Modifiable::No,
            distributable: Distributable::No,
            scope: PendScope::Pages(PendRange::pages(2, 3).unwrap()),
        },
        mark: Mark::new(u64::MAX),
    }
}

#[test]
fn accord_and_narrow_snapshots_have_exact_checked_little_endian_layouts() {
    assert_eq!(AccordRequest::WIDTH, 104);
    assert_eq!(NarrowRequest::WIDTH, 80);
    assert_eq!(
        fetch_words::<13>(&request().to_bytes()).unwrap(),
        [17, 0, 0, usize::MAX, 2, 4, 0, 0, 1, 0, 1, 2, 3]
    );
    let mut value = request();
    for target in [AccordTarget::Caller, AccordTarget::Task(TaskId::new(23))] {
        for range in [PendRange::Inherit, PendRange::pages(2, 3).unwrap()] {
            value.target = target;
            value.limits.scope = PendScope::Pages(range);
            let bytes = value.to_bytes();
            assert_eq!(&bytes[..8], &17u64.to_le_bytes());
            assert_eq!(&bytes[24..32], &u64::MAX.to_le_bytes());
            assert_eq!(AccordRequest::from_bytes(&bytes), Some(value));
            assert_eq!(AccordRequest::from_bytes(&bytes[..103]), None);
            assert_eq!(AccordRequest::from_bytes(&[0; 105]), None);
            let narrow = NarrowRequest {
                token: value.src,
                limits: value.limits,
            };
            let bytes = narrow.to_bytes();
            assert_eq!(
                fetch_words::<10>(&bytes).unwrap()[1..],
                value.limits.words()
            );
            assert_eq!(NarrowRequest::from_bytes(&bytes), Some(narrow));
            assert_eq!(NarrowRequest::from_bytes(&bytes[..79]), None);
            assert_eq!(NarrowRequest::from_bytes(&[0; 81]), None);
        }
    }
    assert_eq!(AccordRequest::from_bytes(&[0; 88]), None);
    assert_eq!(NarrowRequest::from_bytes(&[0; 64]), None);
}

#[test]
fn accord_rejects_noncanonical_targets_permissions_and_scopes() {
    for (word, invalid) in [
        (0, 0),
        (1, 2),
        (1, 1),
        (2, 23),
        (4, 5),
        (4, 7),
        (4, 99),
        (5, 1u64 << 32),
        (5, 1u64 << 31),
        (6, 2),
        (6, 1u64 << 32),
        (7, 2),
        (8, 0),
        (8, 2),
        (8, 4),
        (9, 1),
        (10, 0),
        (10, 2),
        (11, u64::MAX),
        (12, 0),
        (12, u64::MAX),
    ] {
        let mut bytes = request().to_bytes();
        bytes[word * 8..(word + 1) * 8].copy_from_slice(&invalid.to_le_bytes());
        assert_eq!(
            AccordRequest::from_bytes(&bytes),
            None,
            "word {word}, value {invalid}"
        );
        if word >= 4 {
            let narrow = NarrowRequest {
                token: request().src,
                limits: request().limits,
            };
            let mut bytes = narrow.to_bytes();
            bytes[(word - 3) * 8..(word - 2) * 8].copy_from_slice(&invalid.to_le_bytes());
            assert_eq!(
                NarrowRequest::from_bytes(&bytes),
                None,
                "narrow word {}",
                word - 3
            );
        }
    }
    let mut bytes = NarrowRequest {
        token: request().src,
        limits: request().limits,
    }
    .to_bytes();
    bytes[..8].fill(0);
    assert_eq!(NarrowRequest::from_bytes(&bytes), None);
}

#[test]
fn calls_keep_their_slots_and_do_not_reinterpret_retired_pend_registers() {
    let accord = Accord {
        request: VirtAddr::new(0x4000),
    };
    assert_eq!(accord.pack(), [0x4000, 0, 0, 0, 0, 0]);
    assert_eq!(
        PendCall::from_wire(Accord::SLOT, &accord.pack()),
        Ok(accord.into())
    );
    let narrow = Narrow {
        request: VirtAddr::new(0x5000),
    };
    assert_eq!(narrow.pack(), [0x5000, 0, 0, 0, 0, 0]);
    assert_eq!(
        PendCall::from_wire(Narrow::SLOT, &narrow.pack()),
        Ok(narrow.into())
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
                EnvCall::from_wire(kernel_api::abi::PREFIX | (7usize << 32) | slot, &words),
                Err(Decode::BadSlot)
            );
        }
    }
}
