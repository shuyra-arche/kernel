//! Native ABI 14 Flat/Nest domain calls. The register framing and result
//! transport are shared; operation and request identities stay domain-local.
use super::{Opcode, Request, RequestId, journal::RequestKey};

macro_rules! domain {
    ($module:ident, $domain:ident) => {
        pub mod $module {
            use super::*;
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub struct Call(super::super::$module::Request);
            impl Call {
                pub fn new(
                    id: Option<RequestId>,
                    action: super::super::$module::Action,
                ) -> Option<Self> {
                    super::super::$module::Request::new(id, action).map(Self)
                }
                pub const fn from_request(request: super::super::$module::Request) -> Self {
                    Self(request)
                }
                pub const fn request(self) -> super::super::$module::Request {
                    self.0
                }
            }
            impl From<Call> for super::Call {
                fn from(call: Call) -> Self {
                    Self::$domain(call)
                }
            }
        }
    };
}
domain!(flat, Flat);
domain!(nest, Nest);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Call {
    Flat(flat::Call),
    Nest(nest::Call),
}

impl Call {
    /// Bridge for older callers constructing a mixed, checked request.
    /// Real entry and Client transport use only domain-local requests.
    pub fn from_request(request: Request) -> Self {
        let action = request.action();
        match action.opcode() {
            Opcode::Flat(_) => Self::Flat(
                flat::Call::new(
                    request.id(),
                    super::flat::Action::try_from(action).expect("checked Flat action"),
                )
                .expect("checked Flat request"),
            ),
            Opcode::Nest(_) => Self::Nest(
                nest::Call::new(
                    request.id(),
                    super::nest::Action::try_from(action).expect("checked Nest action"),
                )
                .expect("checked Nest request"),
            ),
        }
    }

    pub const fn from_domain(request: DomainCall) -> Self {
        match request {
            DomainCall::Flat(request) => Self::Flat(flat::Call::from_request(request)),
            DomainCall::Nest(request) => Self::Nest(nest::Call::from_request(request)),
        }
    }

    pub const fn request(self) -> DomainCall {
        match self {
            Self::Flat(call) => DomainCall::Flat(call.request()),
            Self::Nest(call) => DomainCall::Nest(call.request()),
        }
    }

    pub const fn number(self) -> u64 {
        self.request().number()
    }
}

/// Validates the register envelope and binds the copied descriptor to a trap
/// number. Caller-local authorization is checked only at the Server commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    opcode: Opcode,
    input: usize,
    output: usize,
}
impl Frame {
    pub fn new(call: Call, input: usize, output: usize) -> Option<Self> {
        Self::decode(call.number(), [input, output, 0, 0, 0, 0])
    }
    pub fn decode(number: u64, registers: [usize; 6]) -> Option<Self> {
        let [input, output, 0, 0, 0, 0] = registers else {
            return None;
        };
        input.checked_add(Request::WIDTH)?;
        output.checked_add(super::Reply::WIDTH)?;
        Some(Self {
            opcode: Opcode::from_call_number(number)?,
            input,
            output,
        })
    }
    pub const fn number(self) -> u64 {
        self.opcode.call_number()
    }
    pub const fn input(self) -> usize {
        self.input
    }
    pub const fn output(self) -> usize {
        self.output
    }
    pub const fn registers(self) -> [usize; 6] {
        [self.input, self.output, 0, 0, 0, 0]
    }
    pub fn domain_snapshot(self, bytes: &[u8]) -> Option<DomainCall> {
        DomainCall::from_call(self.number(), bytes)
    }
    pub fn snapshot(self, bytes: &[u8]) -> Option<Call> {
        self.domain_snapshot(bytes).map(Call::from_domain)
    }
}

/// Validated domain request, retaining its action without a mixed Action copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainCall {
    Flat(super::flat::Request),
    Nest(super::nest::Request),
}
impl DomainCall {
    pub fn from_call(number: u64, bytes: &[u8]) -> Option<Self> {
        match Opcode::from_call_number(number)? {
            Opcode::Flat(_) => super::flat::Request::from_call(number, bytes).map(Self::Flat),
            Opcode::Nest(_) => super::nest::Request::from_call(number, bytes).map(Self::Nest),
        }
    }
    pub const fn number(self) -> u64 {
        match self {
            Self::Flat(request) => Opcode::Flat(request.action().opcode()).call_number(),
            Self::Nest(request) => Opcode::Nest(request.action().opcode()).call_number(),
        }
    }
    pub fn to_bytes(self) -> [u8; Request::WIDTH] {
        match self {
            Self::Flat(request) => request.to_bytes(),
            Self::Nest(request) => request.to_bytes(),
        }
    }
    pub fn key(self) -> Option<RequestKey> {
        match self {
            Self::Flat(request) => RequestKey::from_parts(
                request.id(),
                Opcode::Flat(request.action().opcode()),
                request.to_bytes(),
            ),
            Self::Nest(request) => RequestKey::from_parts(
                request.id(),
                Opcode::Nest(request.action().opcode()),
                request.to_bytes(),
            ),
        }
    }
}
