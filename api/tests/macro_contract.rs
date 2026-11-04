use mold::{Frame, WireCodes};
use wire::{Message, OK};

#[derive(Clone, Copy, Debug, PartialEq, Eq, env::Frame)]
pub struct Ping {
    pub value: u32,
}

impl wire::Message for Ping {
    type In = Self;
    type Buf = [u8; Self::LEN];
    const EMPTY: Self::Buf = [0; Self::LEN];

    fn store(&self, out: &mut [u8]) -> Option<usize> {
        <Self as env::wire::Span>::store_at(self, out, 0)
    }

    fn fetch(bytes: &[u8]) -> Option<Self> {
        let (value, end) = <Self as env::wire::Span>::fetch_at(bytes, 0)?;
        (end == bytes.len()).then_some(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Frame)]
pub struct Pong {
    pub value: u64,
}

impl wire::Message for Pong {
    type In = Self;
    type Buf = [u8; Self::LEN];
    const EMPTY: Self::Buf = [0; Self::LEN];

    fn store(&self, out: &mut [u8]) -> Option<usize> {
        <Self as env::wire::Span>::store_at(self, out, 0)
    }

    fn fetch(bytes: &[u8]) -> Option<Self> {
        let (value, end) = <Self as env::wire::Span>::fetch_at(bytes, 0)?;
        (end == bytes.len()).then_some(value)
    }
}

#[mold::contract(request = Ping, response = Pong)]
pub struct PingContract;

#[derive(Clone, Copy, Debug, PartialEq, Eq, WireCodes)]
#[wire(also(BAD = 4))]
pub enum TinyFail {
    #[code(1)]
    Unknown,
    #[code(2)]
    Taken,
}

#[mold::interface(id = "fixture.v1")]
pub mod api {
    #[channels]
    pub enum Channel {
        #[channel(key = "reply")]
        Reply,
    }

    #[grants]
    pub enum Grant {
        #[grant(code = 1, key = "read")]
        Read,
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    #[requests]
    pub enum Wire {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        #[operation(code = 1, frame = Ask, grant = Read)]
        Ask { value: u64 },
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    #[reply]
    pub struct ReplyFrame {
        pub status: u8,
        pub value: u64,
    }
}

#[test]
fn frame_and_contract_macros_use_compatibility_adapters() {
    fn assert_contract<T: wire::Contract<Request = Ping, Response = Pong>>() {}
    fn assert_message<T: wire::Message>() {}

    assert_contract::<PingContract>();
    assert_message::<Ping>();
    assert_message::<Pong>();

    let ping = Ping { value: 0x1234_5678 };
    let mut bytes = [0u8; Ping::LEN];
    let end = ping.store(&mut bytes).unwrap();
    assert_eq!(&bytes[..end], &0x1234_5678u32.to_le_bytes());
    assert_eq!(Ping::fetch(&bytes[..end]), Some(ping));
}

#[test]
fn wire_codes_macro_keeps_the_domain_code_table() {
    assert_eq!(UNKNOWN, 1);
    assert_eq!(TAKEN, 2);
    assert_eq!(BAD, 4);
    assert_eq!(fail_to_code(Some(TinyFail::Unknown)), UNKNOWN);
    assert_eq!(code_to_fail(UNKNOWN), Some(TinyFail::Unknown));
    assert_eq!(code_to_fail(OK), None);
    assert_eq!(code_to_fail(BAD), None);
}

#[test]
fn interface_macro_uses_compatibility_adapters() {
    assert_eq!(api::INTERFACE_ID, "fixture.v1");
    assert_eq!(api::ASK, 1);
    assert_eq!(api::Grant::Read.at(), 1);
    assert_eq!(api::Grant::Read.name(), "read");
    assert_eq!(api::Channel::Reply.mark(), api::REPLY);

    let value = api::Wire::Ask(api::Ask {
        op: api::ASK,
        value: 7,
    });
    let mut bytes = [0u8; api::Wire::LEN];
    let end = value.store(&mut bytes).unwrap();
    assert_eq!(api::Wire::take(&bytes[..end]), Some(value));
    assert_eq!(api::Wire::fetch(&bytes[..end]), Some(value));
    assert_eq!(api::ReplyFrame::LEN, 9);
}
