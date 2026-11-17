//! Span identity is independent of address, revision and permission.
use super::{memo, request::set_word, span_id};
use kernel_api::{
    abi::memory::{Action, Observation, Query, Request, Snapshot},
    address::VirtAddr,
    model::{
        cell::CellLevel,
        identity::PendId,
        nest::{SpanPhase, SpanStatus, SpanVersion},
        sign::SignSource,
    },
};

#[test]
fn reusing_a_range_cannot_match_an_old_span_observation() {
    let old = memo(0);
    let new_range = PendId::mint(23);
    let query = |range| Request::new(None, Action::Inspect(Query::Span { range })).unwrap();
    let status = SpanStatus {
        range: old.range(),
        version: SpanVersion::INITIAL,
        offset: 0,
        count: 1,
        address: VirtAddr::new(0x8000),
        level: CellLevel::BASE,
        access: 2,
        phase: SpanPhase::Open,
    };
    let previous = Observation::new(Snapshot::Span(status)).unwrap();
    let replacement = Observation::new(Snapshot::Span(SpanStatus {
        range: new_range,
        ..status
    }))
    .unwrap();
    assert!(previous.matches(query(old.range())));
    assert!(!previous.matches(query(new_range)));
    assert!(replacement.matches(query(new_range)));
    assert!(!replacement.matches(query(old.range())));
    let bytes = query(old.range()).to_bytes();
    assert_eq!(&bytes[32..48], &[2u64, 13].map(u64::to_le_bytes).concat());
    for version in [10, 11, 13, 15] {
        let mut mixed = bytes;
        set_word(&mut mixed, 0, version);
        assert!(Request::from_bytes(&mixed).is_none());
    }
    let mut absent = bytes;
    set_word(&mut absent, 5, 0);
    assert!(Request::from_bytes(&absent).is_none());
}

#[test]
fn span_sign_identity_keeps_v10_tags_without_creating_authority() {
    let position = memo(0);
    for (tag, source) in [
        (
            9,
            SignSource::SpanRetired {
                token: position.range(),
                span: span_id(),
            },
        ),
        (
            11,
            SignSource::SpanPublished {
                token: position.range(),
                span: span_id(),
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
