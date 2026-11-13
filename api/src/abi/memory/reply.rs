//! Retained mutation results, separate from reply delivery. A failed write to
//! the caller's output buffer never transforms Committed into Rejected.

use super::{Action, Failure, Opcode, Reader, Request, RequestId, VERSION, Writer};
use crate::model::{
    flat::{BlockPhase, BlockRef, SlotVersion},
    nest::MemoGeneration,
};

/// Operation-specific observations. Numeric Page identity is descriptive only;
/// page=0 in a Slot result means Missing, never a newly allocated zero page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value {
    Slot {
        version: SlotVersion,
        page: u64,
    },
    Transfer {
        local: SlotVersion,
        remote: SlotVersion,
        page: u64,
    },
    Memo {
        generation: MemoGeneration,
    },
    Maintenance {
        block: BlockRef,
        phase: BlockPhase,
    },
}

impl Value {
    fn matches(self, opcode: Opcode) -> bool {
        match (self, opcode) {
            (Self::Slot { page, .. }, Opcode::Occupy | Opcode::Vacate) => page <= isize::MAX as u64,
            (Self::Transfer { page, .. }, Opcode::Push | Opcode::Pull) => {
                page > 0 && page <= isize::MAX as u64
            }
            (
                Self::Memo { .. },
                Opcode::Insert
                | Opcode::Remove
                | Opcode::Publish
                | Opcode::Retract
                | Opcode::Protect,
            )
            | (Self::Maintenance { .. }, Opcode::Block | Opcode::Allow) => true,
            _ => false,
        }
    }

    fn write(self, out: &mut Writer<'_>) -> Option<()> {
        match self {
            Self::Slot { version, page } => {
                out.word(1)?;
                out.word(version.get())?;
                out.word(page)
            }
            Self::Transfer {
                local,
                remote,
                page,
            } => {
                out.word(2)?;
                out.word(local.get())?;
                out.word(remote.get())?;
                out.word(page)
            }
            Self::Memo { generation } => {
                out.word(3)?;
                out.word(generation.get())
            }
            Self::Maintenance { block, phase } => {
                out.word(4)?;
                out.bytes(&block.to_bytes())?;
                out.word(phase as u64)
            }
        }
    }

    fn read(input: &mut Reader<'_>) -> Option<Option<Self>> {
        Some(match input.word()? {
            0 => None,
            1 => Some(Self::Slot {
                version: SlotVersion::new(input.word()?)?,
                page: input.word()?,
            }),
            2 => Some(Self::Transfer {
                local: SlotVersion::new(input.word()?)?,
                remote: SlotVersion::new(input.word()?)?,
                page: input.word()?,
            }),
            3 => Some(Self::Memo {
                generation: MemoGeneration::new(input.word()?)?,
            }),
            4 => Some(Self::Maintenance {
                block: BlockRef::from_bytes(input.bytes::<24>()?)?,
                phase: BlockPhase::from_tag(input.word()?)?,
            }),
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// This attempt did not change resource state. RequestMismatch does not
    /// negate an earlier accepted use of the same ID. A retained rejection may
    /// consume a sequence; retry changed work with a new ID.
    Rejected(Failure),
    /// Accepted work remains; never resubmit with a new ID to finish this work.
    Pending(Value),
    Committed(Value),
    /// Accepted work failed. This is not rollback or hardware settlement.
    Failed {
        failure: Failure,
        retained: Option<Value>,
    },
    /// The old result is no longer known; this ID must never execute again.
    Expired,
}

impl Outcome {
    fn valid(self, opcode: Opcode) -> bool {
        if let Self::Expired = self {
            return true;
        }
        if matches!(opcode, Opcode::Inspect(_)) {
            return false;
        }
        match self {
            Self::Rejected(failure) => {
                !matches!(failure, Failure::ResultExpired | Failure::ReplyFault)
            }
            Self::Failed { failure, retained } => {
                !matches!(
                    failure,
                    Failure::ResultExpired | Failure::ReplyFault | Failure::RequestMismatch
                ) && match (opcode, retained) {
                    (
                        Opcode::Occupy
                        | Opcode::Push
                        | Opcode::Pull
                        | Opcode::Insert
                        | Opcode::Remove,
                        None,
                    ) => true,
                    (Opcode::Vacate, Some(value @ Value::Slot { page: 1.., .. })) => {
                        value.matches(opcode)
                    }
                    (
                        Opcode::Publish | Opcode::Retract | Opcode::Protect,
                        None | Some(Value::Memo { .. }),
                    ) => true,
                    (
                        Opcode::Block,
                        Some(Value::Maintenance {
                            phase: BlockPhase::Draining | BlockPhase::Orphaned,
                            ..
                        }),
                    ) => true,
                    (
                        Opcode::Allow,
                        Some(Value::Maintenance {
                            phase: BlockPhase::Releasing | BlockPhase::Orphaned,
                            ..
                        }),
                    ) => true,
                    _ => false,
                }
            }
            Self::Pending(value) => {
                value.matches(opcode)
                    && matches!(
                        (opcode, value),
                        (
                            Opcode::Block,
                            Value::Maintenance {
                                phase: BlockPhase::Draining,
                                ..
                            }
                        ) | (
                            Opcode::Allow,
                            Value::Maintenance {
                                phase: BlockPhase::Releasing,
                                ..
                            }
                        ) | (
                            Opcode::Publish | Opcode::Retract | Opcode::Protect,
                            Value::Memo { .. }
                        ) | (Opcode::Vacate, Value::Slot { page: 1.., .. })
                    )
            }
            Self::Committed(value) => {
                value.matches(opcode)
                    && match (opcode, value) {
                        (Opcode::Occupy, Value::Slot { page, .. }) => page != 0,
                        (Opcode::Vacate, Value::Slot { page, .. }) => page == 0,
                        (Opcode::Block, Value::Maintenance { phase, .. }) => {
                            phase == BlockPhase::Held
                        }
                        (Opcode::Allow, Value::Maintenance { phase, .. }) => {
                            phase == BlockPhase::Closed
                        }
                        _ => true,
                    }
            }
            Self::Expired => true,
        }
    }

    fn fields(self) -> (u64, i64, Option<Value>) {
        match self {
            Self::Rejected(failure) => (0, failure.code(), None),
            Self::Pending(value) => (1, 0, Some(value)),
            Self::Committed(value) => (2, 0, Some(value)),
            Self::Failed { failure, retained } => (3, failure.code(), retained),
            Self::Expired => (4, Failure::ResultExpired.code(), None),
        }
    }
}

/// Register delivery status, independent of the stored operation outcome. Read
/// the caller-memory Reply only after Written. Failed (especially ReplyFault)
/// is not evidence of rollback or nonacceptance; recover using the original ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    Written,
    Failed(Failure),
}

impl Delivery {
    /// RV64 return words a0..a2. Written is [0, 128, 0]; failure is [code, 0, 0].
    pub const fn registers(self) -> [u64; 3] {
        match self {
            Self::Written => [0, Reply::WIDTH as u64, 0],
            Self::Failed(failure) => [failure.code() as u64, 0, 0],
        }
    }

    pub fn from_registers(words: [u64; 3]) -> Option<Self> {
        match words {
            [0, width, 0] if width == Reply::WIDTH as u64 => Some(Self::Written),
            [code, 0, 0] => Some(Self::Failed(Failure::from_code(code as i64)?)),
            _ => None,
        }
    }
}

/// Exactly 128 bytes: [version, domain, operation, request_id, outcome, failure],
/// then a tagged Value and zero tail. Failed and Expired are not Rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    request: RequestId,
    opcode: Opcode,
    outcome: Outcome,
}

impl Reply {
    pub const WIDTH: usize = 128;

    pub fn new(request: RequestId, opcode: Opcode, outcome: Outcome) -> Option<Self> {
        outcome.valid(opcode).then_some(Self {
            request,
            opcode,
            outcome,
        })
    }

    pub const fn request(self) -> RequestId {
        self.request
    }
    pub const fn opcode(self) -> Opcode {
        self.opcode
    }
    pub const fn outcome(self) -> Outcome {
        self.outcome
    }

    /// Correlates delivery, not authority or resource freshness. Inspect returns
    /// the retained mutation's opcode; an expired query need only know its domain.
    pub fn matches(self, request: Request) -> bool {
        match request.action() {
            Action::InspectResult { domain, request } => {
                self.request == request && self.opcode.domain() == domain
            }
            action => Some(self.request) == request.id() && self.opcode == action.opcode(),
        }
    }

    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        self.write(&mut Writer(&mut bytes))
            .expect("bounded memory reply");
        bytes
    }

    fn write(self, out: &mut Writer<'_>) -> Option<()> {
        let (phase, failure, value) = self.outcome.fields();
        for word in [
            VERSION,
            self.opcode.domain() as u64,
            self.opcode.slot(),
            self.request.get(),
            phase,
            failure as u64,
        ] {
            out.word(word)?;
        }
        match value {
            Some(value) => value.write(out),
            None => out.word(0),
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::WIDTH {
            return None;
        }
        let mut input = Reader(bytes);
        if input.word()? != VERSION {
            return None;
        }
        let opcode = Opcode::of(input.word()?, input.word()?)?;
        let request = RequestId::new(input.word()?)?;
        let phase = input.word()?;
        let failure = input.word()? as i64;
        let value = Value::read(&mut input)?;
        input.finish()?;
        let outcome = match (phase, failure, value) {
            (0, code, None) => Outcome::Rejected(Failure::from_code(code)?),
            (1, 0, Some(value)) => Outcome::Pending(value),
            (2, 0, Some(value)) => Outcome::Committed(value),
            (3, code, retained) => Outcome::Failed {
                failure: Failure::from_code(code)?,
                retained,
            },
            (4, -14, None) => Outcome::Expired,
            _ => return None,
        };
        Self::new(request, opcode, outcome)
    }
}
