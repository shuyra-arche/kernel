//! Retained mutation results, separate from reply delivery. A failed write to
//! the caller's output buffer never transforms Committed into Rejected.

use super::{
    Action, Domain, Failure, Opcode, Reader, Request, RequestId, VERSION, Writer, valid_token,
};
use crate::address::{BASE_PAGE_SIZE, VirtAddr};
use crate::model::{
    cell::MappingId,
    flat::{BlockPhase, BlockRef, FlatId, FlatPhase, FlatVersion, SlotVersion},
    identity::PendId,
    nest::{MappingPhase, MappingVersion, MemoGeneration, NestId, NestPhase, NestVersion},
};

/// Operation-specific observations. Numeric Page identity is descriptive only;
/// page=0 in a Slot result means Missing, never a newly allocated zero page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value {
    CreatedFlat {
        token: PendId,
        flat: FlatId,
        version: FlatVersion,
    },
    CreatedNest {
        token: PendId,
        nest: NestId,
        version: NestVersion,
    },
    Reservation {
        token: PendId,
        nest: NestId,
        nest_version: NestVersion,
        mapping: MappingId,
        version: MappingVersion,
        address: VirtAddr,
    },
    FlatLifecycle {
        flat: FlatId,
        version: FlatVersion,
        phase: FlatPhase,
    },
    NestLifecycle {
        nest: NestId,
        version: NestVersion,
        phase: NestPhase,
    },
    MappingLifecycle {
        nest: NestId,
        mapping: MappingId,
        version: MappingVersion,
        phase: MappingPhase,
    },
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
            (Self::CreatedFlat { token, .. }, Opcode::Create(Domain::Flat))
            | (Self::CreatedNest { token, .. }, Opcode::Create(Domain::Nest)) => {
                valid_token(token.get())
            }
            (
                Self::Reservation {
                    token,
                    mapping,
                    address,
                    ..
                },
                Opcode::Reserve,
            ) => {
                valid_token(token.get())
                    && valid_token(mapping.get())
                    && address.get().is_multiple_of(BASE_PAGE_SIZE)
                    && address.get().checked_add(BASE_PAGE_SIZE).is_some()
            }
            (Self::FlatLifecycle { .. }, Opcode::Delete(Domain::Flat))
            | (Self::NestLifecycle { .. }, Opcode::Delete(Domain::Nest)) => true,
            (Self::MappingLifecycle { mapping, .. }, Opcode::Release) => valid_token(mapping.get()),
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
            Self::CreatedFlat {
                token,
                flat,
                version,
            } => {
                out.word(5)?;
                out.word(token.get() as u64)?;
                out.word(flat.get() as u64)?;
                out.word(version.get())
            }
            Self::CreatedNest {
                token,
                nest,
                version,
            } => {
                out.word(6)?;
                out.word(token.get() as u64)?;
                out.word(nest.get() as u64)?;
                out.word(version.get())
            }
            Self::Reservation {
                token,
                nest,
                nest_version,
                mapping,
                version,
                address,
            } => {
                out.word(7)?;
                out.word(token.get() as u64)?;
                out.word(nest.get() as u64)?;
                out.word(nest_version.get())?;
                out.word(mapping.get() as u64)?;
                out.word(version.get())?;
                out.word(address.get() as u64)
            }
            Self::FlatLifecycle {
                flat,
                version,
                phase,
            } => {
                out.word(8)?;
                out.word(flat.get() as u64)?;
                out.word(version.get())?;
                out.word(phase as u64)
            }
            Self::NestLifecycle {
                nest,
                version,
                phase,
            } => {
                out.word(9)?;
                out.word(nest.get() as u64)?;
                out.word(version.get())?;
                out.word(phase as u64)
            }
            Self::MappingLifecycle {
                nest,
                mapping,
                version,
                phase,
            } => {
                out.word(10)?;
                out.word(nest.get() as u64)?;
                out.word(mapping.get() as u64)?;
                out.word(version.get())?;
                out.word(phase as u64)
            }
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
            5 => Some(Self::CreatedFlat {
                token: input.token()?,
                flat: FlatId::new(input.size()?)?,
                version: FlatVersion::new(input.word()?)?,
            }),
            6 => Some(Self::CreatedNest {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
                version: NestVersion::new(input.word()?)?,
            }),
            7 => Some(Self::Reservation {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
                nest_version: NestVersion::new(input.word()?)?,
                mapping: input.mapping()?,
                version: MappingVersion::new(input.word()?)?,
                address: VirtAddr::new(input.size()?),
            }),
            8 => Some(Self::FlatLifecycle {
                flat: FlatId::new(input.size()?)?,
                version: FlatVersion::new(input.word()?)?,
                phase: FlatPhase::from_tag(input.word()?)?,
            }),
            9 => Some(Self::NestLifecycle {
                nest: NestId::new(input.size()?)?,
                version: NestVersion::new(input.word()?)?,
                phase: match input.word()? {
                    0 => NestPhase::Unbound,
                    1 => NestPhase::Bound,
                    2 => NestPhase::Closing,
                    3 => NestPhase::Closed,
                    4 => NestPhase::Failed,
                    _ => return None,
                },
            }),
            10 => Some(Self::MappingLifecycle {
                nest: NestId::new(input.size()?)?,
                mapping: input.mapping()?,
                version: MappingVersion::new(input.word()?)?,
                phase: MappingPhase::from_tag(input.word()?)?,
            }),
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
                    (Opcode::Create(_) | Opcode::Reserve, None) => true,
                    (
                        Opcode::Delete(Domain::Flat),
                        Some(Value::FlatLifecycle {
                            phase: FlatPhase::Closing | FlatPhase::Failed,
                            ..
                        }),
                    ) => true,
                    (
                        Opcode::Delete(Domain::Nest),
                        Some(Value::NestLifecycle {
                            phase: NestPhase::Closing | NestPhase::Failed,
                            ..
                        }),
                    ) => true,
                    (
                        Opcode::Release,
                        Some(
                            value @ Value::MappingLifecycle {
                                phase: MappingPhase::Releasing | MappingPhase::Failed,
                                ..
                            },
                        ),
                    ) => value.matches(opcode),
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
                            Opcode::Delete(Domain::Flat),
                            Value::FlatLifecycle {
                                phase: FlatPhase::Closing,
                                ..
                            }
                        ) | (
                            Opcode::Delete(Domain::Nest),
                            Value::NestLifecycle {
                                phase: NestPhase::Closing,
                                ..
                            }
                        ) | (
                            Opcode::Release,
                            Value::MappingLifecycle {
                                phase: MappingPhase::Releasing,
                                ..
                            }
                        ) | (
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
                        (Opcode::Delete(Domain::Flat), Value::FlatLifecycle { phase, .. }) => {
                            phase == FlatPhase::Closed
                        }
                        (Opcode::Delete(Domain::Nest), Value::NestLifecycle { phase, .. }) => {
                            phase == NestPhase::Closed
                        }
                        (Opcode::Release, Value::MappingLifecycle { phase, .. }) => {
                            phase == MappingPhase::Released
                        }
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
/// the caller-memory Reply/Observation only after Written. Failed (especially ReplyFault)
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
