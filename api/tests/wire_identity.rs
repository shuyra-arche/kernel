use kernel_api::wire as api_wire;
use wire as old_wire;

fn shared_field<T: old_wire::Field + api_wire::Field>() {}
fn shared_span<T: old_wire::Span + api_wire::Span>() {}
fn shared_message<T: old_wire::Message + api_wire::Message>() {}
fn shared_contract<T: old_wire::Contract + api_wire::Contract>() {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ping(u32);

impl old_wire::Message for Ping {
    type In = Ping;
    type Buf = [u8; 4];
    const EMPTY: Self::Buf = [0; 4];

    fn store(&self, out: &mut [u8]) -> Option<usize> {
        out.get_mut(..4)?.copy_from_slice(&self.0.to_le_bytes());
        Some(4)
    }

    fn fetch(bytes: &[u8]) -> Option<Self::In> {
        Some(Ping(u32::from_le_bytes(
            bytes.get(..4)?.try_into().ok()?,
        )))
    }
}

impl old_wire::Contract for Ping {
    type Request = Ping;
    type Response = Ping;
}

#[test]
fn candidate_wire_paths_share_the_single_wire_trait_family() {
    shared_field::<u32>();
    shared_field::<bool>();
    shared_field::<env::TaskId>();
    shared_field::<env::TeamId>();
    shared_field::<env::PieToken>();
    shared_field::<env::Mark>();

    shared_span::<u32>();
    shared_span::<String>();

    shared_message::<Ping>();
    shared_contract::<Ping>();

    let mut bytes = [0u8; 4];
    api_wire::Field::store(&0x1234_5678u32, &mut bytes);
    assert_eq!(bytes, [0x78, 0x56, 0x34, 0x12]);
    assert_eq!(old_wire::Field::fetch(&bytes), Some(0x1234_5678u32));
    assert_eq!(
        <u32 as api_wire::Field>::fetch(&[0x78, 0x56, 0x34]),
        None
    );
}
