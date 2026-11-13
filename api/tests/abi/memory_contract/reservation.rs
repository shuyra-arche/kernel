//! Reservation identity is independent of address, revision and permission.
use super::{memo, request::set_word};
use kernel_api::{
    abi::memory::{Action, Observation, Query, Request, Snapshot},
    address::VirtAddr,
    model::{
        cell::CellLevel,
        nest::{
            MemoPosition, ReservationId, ReservationPhase, ReservationStatus, ReservationVersion,
        },
        sign::SignSource,
    },
};

#[test]
fn reusing_an_address_and_revision_cannot_match_an_old_reservation_observation() {
    let old = memo(0);
    let new_id = ReservationId::from_bytes(&23u64.to_le_bytes()).unwrap();
    let current = MemoPosition::new(old.token(), old.nest(), new_id, 0).unwrap();
    let query = |position: MemoPosition| {
        Request::new(
            None,
            Action::Inspect(Query::Reservation {
                token: position.token(),
                nest: position.nest(),
                reservation: position.reservation(),
            }),
        )
        .unwrap()
    };
    let status = ReservationStatus {
        nest: old.nest(),
        reservation: old.reservation(),
        version: ReservationVersion::INITIAL,
        offset: 0,
        count: 1,
        address: VirtAddr::new(0x8000),
        level: CellLevel::BASE,
        access: 2,
        phase: ReservationPhase::Open,
    };
    let previous = Observation::new(Snapshot::Reservation(status)).unwrap();
    let replacement = Observation::new(Snapshot::Reservation(ReservationStatus {
        reservation: current.reservation(),
        ..status
    }))
    .unwrap();
    assert!(previous.matches(query(old)));
    assert!(!previous.matches(query(current)));
    assert!(replacement.matches(query(current)));
    assert!(!replacement.matches(query(old)));
    // The rename does not change the versioned descriptor's numeric layout.
    let bytes = query(old).to_bytes();
    assert_eq!(
        &bytes[32..64],
        &[2u64, 13, 17, 19].map(u64::to_le_bytes).concat()
    );
    for version in [10, 12] {
        let mut mixed = bytes;
        set_word(&mut mixed, 0, version);
        assert!(Request::from_bytes(&mixed).is_none());
    }
    let mut absent = bytes;
    set_word(&mut absent, 7, 0);
    assert!(Request::from_bytes(&absent).is_none());
}

#[test]
fn reservation_sign_identity_keeps_v10_tags_without_creating_authority() {
    let position = memo(0);
    for (tag, source) in [
        (
            9,
            SignSource::ReservationRetired {
                token: position.token(),
                reservation: position.reservation(),
            },
        ),
        (
            11,
            SignSource::ReservationPublished {
                token: position.token(),
                reservation: position.reservation(),
            },
        ),
    ] {
        assert_eq!(source.words(), [tag, 13, 19]);
        assert_eq!(SignSource::of([tag, 13, 19]), Some(source));
        for words in [[tag, 0, 19], [tag, 13, 0], [tag, 13, usize::MAX]] {
            assert!(SignSource::of(words).is_none());
        }
    }
}
