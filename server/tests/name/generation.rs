include!("../../src/basis/name/generation.rs");

pub(super) fn last() -> Generation {
    Generation(u64::MAX)
}

#[test]
fn generations_never_wrap_into_a_stale_revision() {
    assert_eq!(Generation::first().get(), 1);
    assert_eq!(Generation::first().next().unwrap().get(), 2);
    let last = Generation(u64::MAX - 1).next().unwrap();
    assert_eq!(last.get(), u64::MAX);
    assert_eq!(last.next(), None);
    assert_eq!(last.next(), None);
}
