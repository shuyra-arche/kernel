//! Retained mutation results, separate from reply delivery. A failed write to
//! the caller's output buffer never transforms Committed into Rejected.

use super::{
    Action, Failure, Opcode, Reader, Request, RequestId, VERSION, Writer, flat as domain_flat,
    nest as domain_nest, valid_token,
};
use crate::address::{BASE_PAGE_SIZE, VirtAddr};
use crate::model::{
    cell::CellLevel,
    flat::{BlockPhase, BlockRef, FlatId, FlatPhase, FlatVersion, Reshape, SlotVersion},
    identity::PendId,
    nest::SpanId,
    nest::{
        FaultEpoch, FaultPhase, MemoGeneration, NestId, NestPhase, NestVersion, SpanPhase,
        SpanVersion,
    },
};

/// Operation-specific observations. Numeric Page identity is descriptive only;
/// page=0 in a Slot result means Missing, never a newly allocated zero page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value {
    FaultRoute {
        nest: NestId,
        span: SpanId,
        version: SpanVersion,
        epoch: FaultEpoch,
        phase: FaultPhase,
    },
    /// One atomic reshape. All resulting heads receive `slots`, equal to the
    /// committed Flat revision. Page IDs must be observed under live authority.
    Geometry {
        flat: FlatId,
        version: FlatVersion,
        slots: SlotVersion,
        range: Reshape,
    },
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
    Span {
        token: PendId,
        nest: NestId,
        nest_version: NestVersion,
        span: SpanId,
        version: SpanVersion,
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
    SpanLifecycle {
        nest: NestId,
        span: SpanId,
        version: SpanVersion,
        phase: SpanPhase,
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
        match opcode {
            Opcode::Flat(opcode) => {
                flat::Value::try_from(self).is_ok_and(|value| value.matches(opcode))
            }
            Opcode::Nest(opcode) => {
                nest::Value::try_from(self).is_ok_and(|value| value.matches(opcode))
            }
        }
    }

    fn write(self, out: &mut Writer<'_>) -> Option<()> {
        match self {
            Self::FaultRoute {
                nest,
                span,
                version,
                epoch,
                phase,
            } => {
                for word in [
                    12,
                    nest.get() as u64,
                    span.get() as u64,
                    version.get(),
                    epoch.get(),
                    phase as u64,
                ] {
                    out.word(word)?;
                }
                Some(())
            }
            Self::Geometry {
                flat,
                version,
                slots,
                range,
            } => {
                out.word(11)?;
                out.word(flat.get() as u64)?;
                out.word(version.get())?;
                out.word(slots.get())?;
                out.word(range.offset() as u64)?;
                out.word(range.parent().get() as u64)
            }
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
            Self::Span {
                token,
                nest,
                nest_version,
                span,
                version,
                address,
            } => {
                out.word(7)?;
                out.word(token.get() as u64)?;
                out.word(nest.get() as u64)?;
                out.word(nest_version.get())?;
                out.word(span.get() as u64)?;
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
            Self::SpanLifecycle {
                nest,
                span,
                version,
                phase,
            } => {
                out.word(10)?;
                out.word(nest.get() as u64)?;
                out.word(span.get() as u64)?;
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
            12 => Some(Self::FaultRoute {
                nest: NestId::new(input.size()?)?,
                span: input.span()?,
                version: SpanVersion::new(input.word()?)?,
                epoch: FaultEpoch::new(input.word()?)?,
                phase: FaultPhase::from_tag(input.word()?)?,
            }),
            11 => Some(Self::Geometry {
                flat: FlatId::new(input.size()?)?,
                version: FlatVersion::new(input.word()?)?,
                slots: SlotVersion::new(input.word()?)?,
                range: Reshape::new(input.size()?, CellLevel::new(input.size()?)?)?,
            }),
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
            7 => Some(Self::Span {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
                nest_version: NestVersion::new(input.word()?)?,
                span: input.span()?,
                version: SpanVersion::new(input.word()?)?,
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
            10 => Some(Self::SpanLifecycle {
                nest: NestId::new(input.size()?)?,
                span: input.span()?,
                version: SpanVersion::new(input.word()?)?,
                phase: SpanPhase::from_tag(input.word()?)?,
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
        if matches!(
            opcode,
            Opcode::Flat(domain_flat::Opcode::Inspect) | Opcode::Nest(domain_nest::Opcode::Inspect)
        ) {
            return false;
        }
        match self {
            Self::Rejected(failure) => !matches!(
                failure,
                Failure::ResultExpired | Failure::ReplyFault | Failure::FaultExpired
            ),
            Self::Failed { failure, retained } => {
                !matches!(
                    failure,
                    Failure::ResultExpired
                        | Failure::ReplyFault
                        | Failure::RequestMismatch
                        | Failure::FaultExpired
                ) && match (opcode, retained) {
                    (
                        Opcode::Flat(domain_flat::Opcode::Create)
                        | Opcode::Nest(domain_nest::Opcode::Create)
                        | Opcode::Nest(domain_nest::Opcode::Reserve)
                        | Opcode::Nest(domain_nest::Opcode::FaultRoute),
                        None,
                    ) => true,
                    (
                        Opcode::Flat(domain_flat::Opcode::Delete),
                        Some(Value::FlatLifecycle {
                            phase: FlatPhase::Closing | FlatPhase::Failed,
                            ..
                        }),
                    ) => true,
                    (
                        Opcode::Nest(domain_nest::Opcode::Delete),
                        Some(Value::NestLifecycle {
                            phase: NestPhase::Closing | NestPhase::Failed,
                            ..
                        }),
                    ) => true,
                    (
                        Opcode::Nest(domain_nest::Opcode::Release),
                        Some(
                            value @ Value::SpanLifecycle {
                                phase: SpanPhase::Closing | SpanPhase::Failed,
                                ..
                            },
                        ),
                    ) => value.matches(opcode),
                    (
                        Opcode::Flat(domain_flat::Opcode::Occupy)
                        | Opcode::Flat(domain_flat::Opcode::Push)
                        | Opcode::Flat(domain_flat::Opcode::Pull)
                        | Opcode::Flat(domain_flat::Opcode::Split)
                        | Opcode::Flat(domain_flat::Opcode::Merge)
                        | Opcode::Nest(domain_nest::Opcode::Insert)
                        | Opcode::Nest(domain_nest::Opcode::Remove),
                        None,
                    ) => true,
                    (
                        Opcode::Flat(domain_flat::Opcode::Vacate),
                        Some(value @ Value::Slot { page: 1.., .. }),
                    ) => value.matches(opcode),
                    (
                        Opcode::Nest(domain_nest::Opcode::Publish)
                        | Opcode::Nest(domain_nest::Opcode::Retract)
                        | Opcode::Nest(domain_nest::Opcode::Protect),
                        None | Some(Value::Memo { .. }),
                    ) => true,
                    (
                        Opcode::Flat(domain_flat::Opcode::Block),
                        Some(Value::Maintenance {
                            phase: BlockPhase::Draining | BlockPhase::Orphaned,
                            ..
                        }),
                    ) => true,
                    (
                        Opcode::Flat(domain_flat::Opcode::Allow),
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
                            Opcode::Flat(domain_flat::Opcode::Delete),
                            Value::FlatLifecycle {
                                phase: FlatPhase::Closing,
                                ..
                            }
                        ) | (
                            Opcode::Nest(domain_nest::Opcode::Delete),
                            Value::NestLifecycle {
                                phase: NestPhase::Closing,
                                ..
                            }
                        ) | (
                            Opcode::Nest(domain_nest::Opcode::Release),
                            Value::SpanLifecycle {
                                phase: SpanPhase::Closing,
                                ..
                            }
                        ) | (
                            Opcode::Flat(domain_flat::Opcode::Block),
                            Value::Maintenance {
                                phase: BlockPhase::Draining,
                                ..
                            }
                        ) | (
                            Opcode::Flat(domain_flat::Opcode::Allow),
                            Value::Maintenance {
                                phase: BlockPhase::Releasing,
                                ..
                            }
                        ) | (
                            Opcode::Nest(domain_nest::Opcode::Publish)
                                | Opcode::Nest(domain_nest::Opcode::Retract)
                                | Opcode::Nest(domain_nest::Opcode::Protect),
                            Value::Memo { .. }
                        ) | (
                            Opcode::Flat(domain_flat::Opcode::Vacate),
                            Value::Slot { page: 1.., .. }
                        )
                    )
            }
            Self::Committed(value) => {
                value.matches(opcode)
                    && match (opcode, value) {
                        (
                            Opcode::Flat(domain_flat::Opcode::Delete),
                            Value::FlatLifecycle { phase, .. },
                        ) => phase == FlatPhase::Closed,
                        (
                            Opcode::Nest(domain_nest::Opcode::Delete),
                            Value::NestLifecycle { phase, .. },
                        ) => phase == NestPhase::Closed,
                        (
                            Opcode::Nest(domain_nest::Opcode::Release),
                            Value::SpanLifecycle { phase, .. },
                        ) => phase == SpanPhase::Closed,
                        (
                            Opcode::Nest(domain_nest::Opcode::ReleaseRange),
                            Value::SpanLifecycle { phase, .. },
                        ) => phase == SpanPhase::Open,
                        (Opcode::Flat(domain_flat::Opcode::Occupy), Value::Slot { page, .. }) => {
                            page != 0
                        }
                        (Opcode::Flat(domain_flat::Opcode::Vacate), Value::Slot { page, .. }) => {
                            page == 0
                        }
                        (
                            Opcode::Flat(domain_flat::Opcode::Block),
                            Value::Maintenance { phase, .. },
                        ) => phase == BlockPhase::Held,
                        (
                            Opcode::Flat(domain_flat::Opcode::Allow),
                            Value::Maintenance { phase, .. },
                        ) => phase == BlockPhase::Closed,
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
            action => {
                if Some(self.request) != request.id() || self.opcode != action.opcode() {
                    return false;
                }
                match (action, self.outcome) {
                    (
                        Action::FaultRoute {
                            nest,
                            span,
                            expected,
                            epoch,
                            route,
                            ..
                        },
                        Outcome::Committed(Value::FaultRoute {
                            nest: actual_nest,
                            span: actual_span,
                            version,
                            epoch: actual_epoch,
                            phase,
                        }),
                    ) => {
                        nest == actual_nest
                            && span == actual_span
                            && expected.next() == Some(version)
                            && epoch.next() == Some(actual_epoch)
                            && phase
                                == if route.is_some() {
                                    FaultPhase::Active
                                } else {
                                    FaultPhase::Disabled
                                }
                    }
                    (
                        Action::Split {
                            start,
                            expected,
                            parent,
                            ..
                        }
                        | Action::Merge {
                            start,
                            expected,
                            parent,
                            ..
                        },
                        Outcome::Committed(Value::Geometry {
                            flat,
                            version,
                            range,
                            ..
                        }),
                    ) => {
                        flat == start.flat()
                            && range.offset() == start.page()
                            && range.parent() == parent
                            && expected.next() == Some(version)
                    }
                    _ => true,
                }
            }
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

/// Flat-domain results.
pub mod flat {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Value {
        Geometry {
            flat: FlatId,
            version: FlatVersion,
            slots: SlotVersion,
            range: Reshape,
        },
        Created {
            token: PendId,
            flat: FlatId,
            version: FlatVersion,
        },
        Lifecycle {
            flat: FlatId,
            version: FlatVersion,
            phase: FlatPhase,
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
        Maintenance {
            block: BlockRef,
            phase: BlockPhase,
        },
    }

    impl Value {
        pub(super) fn matches(self, opcode: domain_flat::Opcode) -> bool {
            use domain_flat::Opcode;
            match (self, opcode) {
                (Self::Geometry { version, slots, .. }, Opcode::Split | Opcode::Merge) => {
                    version.get() > 1 && slots.get() == version.get()
                }
                (Self::Created { token, .. }, Opcode::Create) => valid_token(token.get()),
                (Self::Lifecycle { .. }, Opcode::Delete) => true,
                (Self::Slot { page, .. }, Opcode::Occupy | Opcode::Vacate) => {
                    page <= isize::MAX as u64
                }
                (Self::Transfer { page, .. }, Opcode::Push | Opcode::Pull) => {
                    page > 0 && page <= isize::MAX as u64
                }
                (Self::Maintenance { .. }, Opcode::Block | Opcode::Allow) => true,
                _ => false,
            }
        }
    }

    impl From<Value> for super::Value {
        fn from(value: Value) -> Self {
            match value {
                Value::Geometry {
                    flat,
                    version,
                    slots,
                    range,
                } => Self::Geometry {
                    flat,
                    version,
                    slots,
                    range,
                },
                Value::Created {
                    token,
                    flat,
                    version,
                } => Self::CreatedFlat {
                    token,
                    flat,
                    version,
                },
                Value::Lifecycle {
                    flat,
                    version,
                    phase,
                } => Self::FlatLifecycle {
                    flat,
                    version,
                    phase,
                },
                Value::Slot { version, page } => Self::Slot { version, page },
                Value::Transfer {
                    local,
                    remote,
                    page,
                } => Self::Transfer {
                    local,
                    remote,
                    page,
                },
                Value::Maintenance { block, phase } => Self::Maintenance { block, phase },
            }
        }
    }

    impl TryFrom<super::Value> for Value {
        type Error = ();

        fn try_from(value: super::Value) -> Result<Self, Self::Error> {
            Ok(match value {
                super::Value::Geometry {
                    flat,
                    version,
                    slots,
                    range,
                } => Self::Geometry {
                    flat,
                    version,
                    slots,
                    range,
                },
                super::Value::CreatedFlat {
                    token,
                    flat,
                    version,
                } => Self::Created {
                    token,
                    flat,
                    version,
                },
                super::Value::FlatLifecycle {
                    flat,
                    version,
                    phase,
                } => Self::Lifecycle {
                    flat,
                    version,
                    phase,
                },
                super::Value::Slot { version, page } => Self::Slot { version, page },
                super::Value::Transfer {
                    local,
                    remote,
                    page,
                } => Self::Transfer {
                    local,
                    remote,
                    page,
                },
                super::Value::Maintenance { block, phase } => Self::Maintenance { block, phase },
                _ => return Err(()),
            })
        }
    }
}

/// Nest-domain results.
pub mod nest {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Value {
        FaultRoute {
            nest: NestId,
            span: SpanId,
            version: SpanVersion,
            epoch: FaultEpoch,
            phase: FaultPhase,
        },
        Created {
            token: PendId,
            nest: NestId,
            version: NestVersion,
        },
        Span {
            token: PendId,
            nest: NestId,
            nest_version: NestVersion,
            span: SpanId,
            version: SpanVersion,
            address: VirtAddr,
        },
        Lifecycle {
            nest: NestId,
            version: NestVersion,
            phase: NestPhase,
        },
        SpanLifecycle {
            nest: NestId,
            span: SpanId,
            version: SpanVersion,
            phase: SpanPhase,
        },
        Memo {
            generation: MemoGeneration,
        },
    }

    impl Value {
        pub(super) fn matches(self, opcode: domain_nest::Opcode) -> bool {
            use domain_nest::Opcode;
            match (self, opcode) {
                (
                    Self::FaultRoute {
                        span,
                        version,
                        epoch,
                        phase,
                        ..
                    },
                    Opcode::FaultRoute,
                ) => {
                    valid_token(span.get())
                        && version.get() > 1
                        && epoch.get() > 1
                        && matches!(phase, FaultPhase::Active | FaultPhase::Disabled)
                }
                (Self::Created { token, .. }, Opcode::Create) => valid_token(token.get()),
                (
                    Self::Span {
                        token,
                        span,
                        address,
                        ..
                    },
                    Opcode::Reserve,
                ) => {
                    valid_token(token.get())
                        && valid_token(span.get())
                        && address.get().is_multiple_of(BASE_PAGE_SIZE)
                        && address.get().checked_add(BASE_PAGE_SIZE).is_some()
                }
                (Self::Lifecycle { .. }, Opcode::Delete) => true,
                (Self::SpanLifecycle { span, .. }, Opcode::Release | Opcode::ReleaseRange) => {
                    valid_token(span.get())
                }
                (
                    Self::Memo { .. },
                    Opcode::Insert
                    | Opcode::Remove
                    | Opcode::Publish
                    | Opcode::Retract
                    | Opcode::Protect,
                ) => true,
                _ => false,
            }
        }
    }

    impl From<Value> for super::Value {
        fn from(value: Value) -> Self {
            match value {
                Value::FaultRoute {
                    nest,
                    span,
                    version,
                    epoch,
                    phase,
                } => Self::FaultRoute {
                    nest,
                    span,
                    version,
                    epoch,
                    phase,
                },
                Value::Created {
                    token,
                    nest,
                    version,
                } => Self::CreatedNest {
                    token,
                    nest,
                    version,
                },
                Value::Span {
                    token,
                    nest,
                    nest_version,
                    span,
                    version,
                    address,
                } => Self::Span {
                    token,
                    nest,
                    nest_version,
                    span,
                    version,
                    address,
                },
                Value::Lifecycle {
                    nest,
                    version,
                    phase,
                } => Self::NestLifecycle {
                    nest,
                    version,
                    phase,
                },
                Value::SpanLifecycle {
                    nest,
                    span,
                    version,
                    phase,
                } => Self::SpanLifecycle {
                    nest,
                    span,
                    version,
                    phase,
                },
                Value::Memo { generation } => Self::Memo { generation },
            }
        }
    }

    impl TryFrom<super::Value> for Value {
        type Error = ();

        fn try_from(value: super::Value) -> Result<Self, Self::Error> {
            Ok(match value {
                super::Value::FaultRoute {
                    nest,
                    span,
                    version,
                    epoch,
                    phase,
                } => Self::FaultRoute {
                    nest,
                    span,
                    version,
                    epoch,
                    phase,
                },
                super::Value::CreatedNest {
                    token,
                    nest,
                    version,
                } => Self::Created {
                    token,
                    nest,
                    version,
                },
                super::Value::Span {
                    token,
                    nest,
                    nest_version,
                    span,
                    version,
                    address,
                } => Self::Span {
                    token,
                    nest,
                    nest_version,
                    span,
                    version,
                    address,
                },
                super::Value::NestLifecycle {
                    nest,
                    version,
                    phase,
                } => Self::Lifecycle {
                    nest,
                    version,
                    phase,
                },
                super::Value::SpanLifecycle {
                    nest,
                    span,
                    version,
                    phase,
                } => Self::SpanLifecycle {
                    nest,
                    span,
                    version,
                    phase,
                },
                super::Value::Memo { generation } => Self::Memo { generation },
                _ => return Err(()),
            })
        }
    }
}
