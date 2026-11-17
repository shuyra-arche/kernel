//! Span identity is independent of address, revision and permission.
use super::{memo, nest_id, request::set_word, span_id};
use kernel_api::{
    abi::memory::{Action, Observation, Query, Request, Snapshot},
    address::VirtAddr,
    model::{
        cell::CellLevel,
        nest::{MemoPosition, SpanId, SpanPhase, SpanStatus, SpanVersion},
        sign::SignSource,
    },
};

#[test]
fn reusing_an_address_and_revision_cannot_match_an_old_span_observation() {
    let old = memo(0);
    let new_id = SpanId::from_bytes(&23u64.to_le_bytes()).unwrap();
    let current = MemoPosition::new(old.range(), 0).unwrap();
    let query = |span: SpanId| {
        Request::new(
            None,
            Action::Inspect(Query::Span {
                token: old.range(),
                nest: nest_id(),
                span,
            }),
        )
        .unwrap()
    };
    let status = SpanStatus {
        nest: nest_id(),
        span: span_id(),
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
        span: new_id,
        ..status
    }))
    .unwrap();
    assert!(previous.matches(query(span_id())));
    assert!(!previous.matches(query(new_id)));
    assert!(replacement.matches(query(new_id)));
    assert!(!replacement.matches(query(span_id())));
    // The rename does not change the versioned descriptor's numeric layout.
    let bytes = query(span_id()).to_bytes();
    assert_eq!(
        &bytes[32..64],
        &[2u64, 13, 17, 19].map(u64::to_le_bytes).concat()
    );
    for version in [10, 11, 13, 15] {
        let mut mixed = bytes;
        set_word(&mut mixed, 0, version);
        assert!(Request::from_bytes(&mixed).is_none());
    }
    let mut absent = bytes;
    set_word(&mut absent, 7, 0);
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
