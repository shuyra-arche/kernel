use kernel_api::env as api_env;
use kernel_api::env::wire::Field;

#[test]
fn permission_decoding_rejects_unknown_wide_and_mixed_family_bits() {
    assert_eq!(api_env::Permission::from_bits(0b1_0000), None);

    let mut at = 0;
    assert_eq!(
        <api_env::Permission as api_env::Wire>::unpack(&[0b1_0000, 0, 0, 0, 0, 0], &mut at),
        Err(api_env::Decode::Invalid)
    );

    let mut at = 0;
    assert_eq!(
        <api_env::Permission as api_env::Wire>::unpack(&[usize::MAX, 0, 0, 0, 0, 0], &mut at),
        Err(api_env::Decode::Invalid)
    );

    let mixed = (api_env::Permission::FETCH | api_env::Permission::VEST).bits();
    assert_eq!(api_env::Access::from_bits(mixed), None);
    assert_eq!(api_env::Policy::from_bits(mixed), None);
}

#[test]
fn memory_failures_and_unit_slots_keep_the_existing_values() {
    assert_eq!(api_env::MemoryFail::Denied.code(), -1);
    assert_eq!(api_env::MemoryFail::Busy.code(), -7);
    assert!(api_env::MemoryFail::Busy.is_busy());
    assert_eq!(api_env::MemoryFail::NotTransferable.code(), -10);
    assert_eq!(api_env::MemoryFail::of_code(-11), None);
    assert_eq!(api_env::MemoryFail::of_code(0), None);

    let call = api_env::UnitCall::Join {
        target: api_env::UnitTarget::Team(api_env::TeamId::new(7)),
        millis: api_env::Wait::AtMost(123),
        receive: true,
    };
    assert_eq!(call.slot(), (1usize << 32) | 37);
    assert_eq!(
        api_env::EnvCall::from_wire(call.slot(), &call.pack()),
        Ok(api_env::EnvCall::Unit(call))
    );
}

#[test]
fn reference_range_keeps_the_existing_field_width_and_little_endian_bytes() {
    let reference = api_env::FlatReference::from_bytes(&7u64.to_le_bytes()).unwrap();
    let range = api_env::ReferenceRange::new(reference, 1, 2).unwrap();

    assert_eq!(<api_env::ReferenceRange as Field>::WIDTH, 24);

    let mut bytes = [0u8; 24];
    range.store(&mut bytes);
    for (actual, expected) in bytes.chunks_exact(8).zip([7u64, 1, 2]) {
        assert_eq!(actual, expected.to_le_bytes());
    }

    assert_eq!(api_env::ReferenceRange::fetch(&bytes), Some(range));
    assert_eq!(api_env::ReferenceRange::fetch(&bytes[..23]), None);
    assert_eq!(api_env::ReferenceRange::fetch(&[0u8; 25]), None);
}
