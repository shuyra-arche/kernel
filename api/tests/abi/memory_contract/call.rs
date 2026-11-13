use kernel_api::{
    abi::{
        call::EnvCall,
        memory::{self, Action, Call, Domain, Frame, Opcode, RequestId},
    },
    model::{cell::CellLevel, identity::PendId},
};

fn flat() -> memory::flat::Call {
    memory::flat::Call::new(
        RequestId::new(7),
        Action::CreateFlat {
            license: PendId::mint(3),
            count: 512,
            level: CellLevel::BASE,
        },
    )
    .unwrap()
}

#[test]
fn next_residency_rights_do_not_widen_the_v10_decoder() {
    use kernel_api::model::pend::{FlatRights, Operations};
    let rights = memory::rights::OCCUPY | memory::rights::VACATE | memory::rights::RESHAPE;
    assert_eq!(rights.bits(), (1 << 6) | (1 << 7) | (1 << 8));
    assert!(FlatRights::from_bits(rights.bits()).is_none());
    assert!(!FlatRights::all().intersects(rights));
    assert!(Operations::Flat(rights).contains(Operations::Flat(memory::rights::OCCUPY)));
}
#[test]
fn next_surface_has_only_flat_and_nest_and_no_duplicate_create_domain() {
    let flat = flat();
    let nest = memory::nest::Call::new(
        RequestId::new(9),
        Action::CreateNest {
            license: PendId::mint(3),
        },
    )
    .unwrap();
    assert!(memory::nest::Call::from_request(flat.request()).is_none());
    assert!(memory::flat::Call::from_request(nest.request()).is_none());
    for (call, domain) in [
        (Call::from(flat), Domain::Flat),
        (Call::from(nest), Domain::Nest),
    ] {
        assert_eq!(call.number(), (11 << 48) | ((domain as u64) << 32) | 16);
        let frame = Frame::new(call, 0x1001, 0x2003).unwrap();
        assert_eq!(frame.registers(), [0x1001, 0x2003, 0, 0, 0, 0]);
        assert_eq!(frame.snapshot(&call.request().to_bytes()), Some(call));
        assert!(EnvCall::from_wire(call.number() as usize, &frame.registers()).is_err());
    }
    for slot in 0..32 {
        assert!(Frame::decode((11 << 48) | (2 << 32) | slot, [0; 6]).is_none());
    }
}
#[test]
fn register_envelope_rejects_spares_overflow_and_descriptor_substitution() {
    let call = Call::from(flat());
    for word in 2..6 {
        let mut regs = [0; 6];
        regs[word] = 1;
        assert!(Frame::decode(call.number(), regs).is_none());
    }
    assert!(Frame::new(call, usize::MAX - 255, 0).is_none());
    assert!(Frame::new(call, 0, usize::MAX - 127).is_none());
    let frame = Frame::new(call, 8, 8).unwrap(); // overlap is legal; snapshot comes first
    assert_eq!(frame.input(), frame.output());
    let bytes = call.request().to_bytes();
    assert!(frame.snapshot(&bytes[..255]).is_none());
    let other = Frame::decode(Opcode::Delete(Domain::Flat).call_number(), [0; 6]).unwrap();
    assert!(other.snapshot(&bytes).is_none());
    assert!(Frame::decode((10 << 48) | (12 << 32) | 16, [0; 6]).is_none());
}
