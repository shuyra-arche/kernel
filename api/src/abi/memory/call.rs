//! Native ABI 11 has exactly two memory operation domains. The default Cell
//! register adapter is not part of this interface. No alternate creation path
//! or Task-local Slot handle can be encoded here.
use super::{Action, Domain, Opcode, Request, RequestId};

macro_rules! domain {
    ($module:ident, $domain:ident) => {
        pub mod $module {
            use super::*;
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub struct Call(Request);
            impl Call {
                pub fn new(id: Option<RequestId>, action: Action) -> Option<Self> {
                    Self::from_request(Request::new(id, action)?)
                }
                pub fn from_request(request: Request) -> Option<Self> {
                    (request.action().opcode().domain() == Domain::$domain).then_some(Self(request))
                }
                pub const fn request(self) -> Request {
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
    pub fn from_request(request: Request) -> Self {
        match request.action().opcode().domain() {
            Domain::Flat => Self::Flat(flat::Call::from_request(request).unwrap()),
            Domain::Nest => Self::Nest(nest::Call::from_request(request).unwrap()),
        }
    }
    pub const fn request(self) -> Request {
        match self {
            Self::Flat(call) => call.request(),
            Self::Nest(call) => call.request(),
        }
    }
    pub const fn number(self) -> u64 {
        self.request().action().opcode().call_number()
    }
}

/// Checked register envelope. It validates arithmetic, not caller memory or
/// authority. Entry must snapshot all request bytes before effects/output, then
/// use snapshot() to bind the descriptor to the actual trap number.
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
    pub fn snapshot(self, bytes: &[u8]) -> Option<Call> {
        Some(Call::from_request(Request::from_call(
            self.number(),
            bytes,
        )?))
    }
}
