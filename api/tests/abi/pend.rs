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
        pend::{AccordRequest, AccordTarget, PendRange},
    },
};

fn request() -> AccordRequest {
    AccordRequest {
        src: PendId::mint(17),
        target: AccordTarget::Caller,
        subset: Modifiable::No,
        mark: Mark::new(u64::MAX),
        range: PendRange::pages(2, 3).unwrap(),
    }
}

#[test]
fn accord_snapshot_has_a_fixed_checked_little_endian_layout() {
    let mut value = request();
    for target in [AccordTarget::Caller, AccordTarget::Task(TaskId::new(23))] {
        for range in [PendRange::Inherit, PendRange::pages(2, 3).unwrap()] {
            value.target = target;
            value.range = range;
            let bytes = value.to_bytes();
            assert_eq!(bytes.len(), 64);
            assert_eq!(&bytes[..8], &17u64.to_le_bytes());
            assert_eq!(&bytes[32..40], &u64::MAX.to_le_bytes());
            assert_eq!(AccordRequest::from_bytes(&bytes), Some(value));
            assert_eq!(AccordRequest::from_bytes(&bytes[..63]), None);
            assert_eq!(AccordRequest::from_bytes(&[0; 65]), None);
        }
    }
}

#[test]
fn accord_rejects_noncanonical_targets_permissions_and_ranges() {
    for (word, invalid) in [
        (0, 0),
        (1, 2),
        (1, 1),
        (2, 23), // missing source, unknown tag, Task(0), Caller payload
        (3, 2),
        (3, 1u64 << 32), // no permission truncation
        (5, 2),
        (5, 0),
        (6, u64::MAX),
        (7, 0),
        (7, u64::MAX),
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
