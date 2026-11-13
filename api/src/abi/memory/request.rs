//! Bounded caller-memory requests for the settled Flat/Nest transitions. The
//! header names the domain and operation; unused bytes have one canonical zero
//! representation, suitable for comparing duplicate request identities.

use super::{Domain, Opcode, Query, Reader, RequestId, VERSION, Writer, valid_token};
use crate::model::{
    cell::{CellLevel, MappingId},
    flat::{BlockRef, FlatId, FlatVersion, Reshape, SlotPosition, SlotVersion},
    identity::PendId,
    nest::{
        FaultEpoch, FaultRoute, MappingVersion, MemoGeneration, MemoPosition, NestId, NestVersion,
        ReserveRequest,
    },
    view::ViewId,
};

/// A live-grant argument plus a maintenance locator. The codec cannot establish
/// that the caller owns the bound branch/Task or that the record is Held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Maintenance {
    pub token: PendId,
    pub block: BlockRef,
}

impl Maintenance {
    fn valid(self) -> bool {
        valid_token(self.token.get())
    }

    fn write(self, out: &mut Writer<'_>) -> Option<()> {
        out.word(self.token.get() as u64)?;
        out.bytes(&self.block.to_bytes())
    }

    fn read(input: &mut Reader<'_>) -> Option<Self> {
        Some(Self {
            token: input.token()?,
            block: BlockRef::from_bytes(input.bytes::<24>()?)?,
        })
    }
}

/// Logical source selection, never a physical ownership grant. Flat/Memo
/// versions are checked when Insert commits; they are not permanent residency
/// pins. Share coordinates use the original View origin, not a narrowed window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Flat {
        position: SlotPosition,
        expected: SlotVersion,
    },
    Share {
        token: PendId,
        view: ViewId,
        page: usize,
        count: usize,
    },
    Memo {
        position: MemoPosition,
        expected: MemoGeneration,
    },
}

impl Source {
    fn valid(self) -> bool {
        match self {
            Self::Flat { .. } | Self::Memo { .. } => true,
            Self::Share {
                token,
                view,
                page,
                count,
            } => {
                valid_token(token.get())
                    && valid_token(view.get())
                    && crate::model::cell::pages(page, count).is_some()
            }
        }
    }

    fn write(self, out: &mut Writer<'_>) -> Option<()> {
        match self {
            Self::Flat { position, expected } => {
                out.word(0)?;
                write_slot(out, position, expected)
            }
            Self::Share {
                token,
                view,
                page,
                count,
            } => {
                out.word(1)?;
                for word in [token.get(), view.get(), page, count] {
                    out.word(word as u64)?;
                }
                Some(())
            }
            Self::Memo { position, expected } => {
                out.word(2)?;
                write_memo(out, position, expected)
            }
        }
    }

    fn read(input: &mut Reader<'_>) -> Option<Self> {
        Some(match input.word()? {
            0 => {
                let (position, expected) = read_slot(input)?;
                Self::Flat { position, expected }
            }
            1 => Self::Share {
                token: input.token()?,
                view: ViewId::new(input.size()?),
                page: input.size()?,
                count: input.size()?,
            },
            2 => {
                let (position, expected) = read_memo(input)?;
                Self::Memo { position, expected }
            }
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Atomically install/replace or disable a Mapping-owned fault route. The
    /// provider binds the actual caller, never a caller-supplied handler Task.
    FaultRoute {
        token: PendId,
        nest: NestId,
        mapping: MappingId,
        expected: MappingVersion,
        epoch: FaultEpoch,
        route: Option<FaultRoute>,
    },
    CreateFlat {
        license: PendId,
        count: usize,
        level: CellLevel,
    },
    DeleteFlat {
        token: PendId,
        flat: FlatId,
        expected: FlatVersion,
    },
    CreateNest {
        license: PendId,
    },
    DeleteNest {
        token: PendId,
        nest: NestId,
        expected: NestVersion,
    },
    Reserve {
        token: PendId,
        nest: NestId,
        expected: NestVersion,
        reservation: ReserveRequest,
    },
    Release {
        token: PendId,
        nest: NestId,
        mapping: MappingId,
        expected: MappingVersion,
    },
    /// One parent to its 512 immediate children. The expected Flat revision
    /// covers every affected coordinate, not merely the first Slot's stamp.
    Split {
        start: SlotPosition,
        expected: FlatVersion,
        parent: CellLevel,
        maintenance: Maintenance,
    },
    /// The same parent-sized range, from 512 immediate children to one parent.
    Merge {
        start: SlotPosition,
        expected: FlatVersion,
        parent: CellLevel,
        maintenance: Maintenance,
    },
    Inspect(Query),
    /// Query tag 3; resolves only the actual caller's retained result. No live
    /// resource token is necessary after an accepted deletion or revocation.
    InspectResult {
        domain: Domain,
        request: RequestId,
    },
    Occupy {
        slot: SlotPosition,
        expected: SlotVersion,
    },
    Vacate {
        slot: SlotPosition,
        expected: SlotVersion,
        maintenance: Maintenance,
    },
    /// A fixed range; recovery names an existing orphaned record in this Flat.
    Block {
        start: SlotPosition,
        count: usize,
        recover: Option<BlockRef>,
    },
    Allow {
        maintenance: Maintenance,
    },
    Push {
        local: SlotPosition,
        local_version: SlotVersion,
        remote: SlotPosition,
        remote_version: SlotVersion,
        maintenance: Option<Maintenance>,
    },
    Pull {
        local: SlotPosition,
        local_version: SlotVersion,
        remote: SlotPosition,
        remote_version: SlotVersion,
        maintenance: Option<Maintenance>,
    },
    Insert {
        target: MemoPosition,
        expected: MemoGeneration,
        source: Source,
    },
    Remove {
        memo: MemoPosition,
        expected: MemoGeneration,
    },
    Publish {
        memo: MemoPosition,
        expected: MemoGeneration,
        maintenance: Option<Maintenance>,
    },
    Retract {
        memo: MemoPosition,
        expected: MemoGeneration,
    },
    Protect {
        memo: MemoPosition,
        expected: MemoGeneration,
        access: u64,
    },
}

impl Action {
    pub const fn opcode(self) -> Opcode {
        match self {
            Self::FaultRoute { .. } => Opcode::FaultRoute,
            Self::CreateFlat { .. } => Opcode::Create(Domain::Flat),
            Self::DeleteFlat { .. } => Opcode::Delete(Domain::Flat),
            Self::CreateNest { .. } => Opcode::Create(Domain::Nest),
            Self::DeleteNest { .. } => Opcode::Delete(Domain::Nest),
            Self::Reserve { .. } => Opcode::Reserve,
            Self::Release { .. } => Opcode::Release,
            Self::Split { .. } => Opcode::Split,
            Self::Merge { .. } => Opcode::Merge,
            Self::Inspect(query) => Opcode::Inspect(query.domain()),
            Self::InspectResult { domain, .. } => Opcode::Inspect(domain),
            Self::Occupy { .. } => Opcode::Occupy,
            Self::Vacate { .. } => Opcode::Vacate,
            Self::Block { .. } => Opcode::Block,
            Self::Allow { .. } => Opcode::Allow,
            Self::Push { .. } => Opcode::Push,
            Self::Pull { .. } => Opcode::Pull,
            Self::Insert { .. } => Opcode::Insert,
            Self::Remove { .. } => Opcode::Remove,
            Self::Publish { .. } => Opcode::Publish,
            Self::Retract { .. } => Opcode::Retract,
            Self::Protect { .. } => Opcode::Protect,
        }
    }

    fn valid(self) -> bool {
        match self {
            Self::FaultRoute {
                token,
                mapping,
                route,
                ..
            } => {
                valid_token(token.get())
                    && valid_token(mapping.get())
                    && route.is_none_or(FaultRoute::valid)
            }
            Self::CreateFlat {
                license,
                count,
                level,
            } => {
                valid_token(license.get())
                    && crate::model::cell::pages(0, count).is_some()
                    && level
                        .pages()
                        .is_some_and(|grain| count.is_multiple_of(grain))
            }
            Self::CreateNest { license } => valid_token(license.get()),
            Self::DeleteFlat { token, .. } | Self::DeleteNest { token, .. } => {
                valid_token(token.get())
            }
            Self::Reserve {
                token, reservation, ..
            } => {
                valid_token(token.get())
                    && reservation.valid()
                    && match reservation.placement.at() {
                        crate::model::cell::PlacementAt::Exact(address) => reservation
                            .count
                            .checked_bytes()
                            .and_then(|bytes| address.get().checked_add(bytes))
                            .is_some(),
                        _ => true,
                    }
            }
            Self::Release { token, mapping, .. } => {
                valid_token(token.get()) && valid_token(mapping.get())
            }
            Self::Split {
                start,
                parent,
                maintenance,
                ..
            }
            | Self::Merge {
                start,
                parent,
                maintenance,
                ..
            } => {
                Reshape::new(start.page(), parent).is_some()
                    && maintenance.valid()
                    && maintenance.block.flat() == start.flat()
            }
            Self::Inspect(query) => query.valid(),
            Self::Vacate { maintenance, .. } | Self::Allow { maintenance } => maintenance.valid(),
            Self::Block {
                start,
                count,
                recover,
            } => {
                crate::model::cell::pages(start.page(), count).is_some()
                    && recover.is_none_or(|block| block.flat() == start.flat())
            }
            Self::Push {
                local,
                remote,
                maintenance,
                ..
            }
            | Self::Pull {
                local,
                remote,
                maintenance,
                ..
            } => {
                // Different grants to the same position do not create a second holding.
                (local.flat() != remote.flat() || local.page() != remote.page())
                    && maintenance.is_none_or(Maintenance::valid)
            }
            Self::Publish { maintenance, .. } => maintenance.is_none_or(Maintenance::valid),
            Self::Insert { source, .. } => source.valid(),
            Self::Protect { access, .. } => {
                access & !14 == 0 && access & 10 != 0 && (access & 4 == 0 || access & 2 != 0)
            }
            _ => true,
        }
    }

    fn write(self, out: &mut Writer<'_>) -> Option<()> {
        match self {
            Self::FaultRoute {
                token,
                nest,
                mapping,
                expected,
                epoch,
                route,
            } => {
                for word in [
                    token.get() as u64,
                    nest.get() as u64,
                    mapping.get() as u64,
                    expected.get(),
                    epoch.get(),
                    u64::from(route.is_some()),
                ] {
                    out.word(word)?;
                }
                if let Some(route) = route {
                    out.word(route.sign.get() as u64)?;
                    out.word(route.timeout_ns)?;
                }
                Some(())
            }
            Self::CreateFlat {
                license,
                count,
                level,
            } => {
                out.word(license.get() as u64)?;
                out.word(count as u64)?;
                out.word(level.get() as u64)
            }
            Self::DeleteFlat {
                token,
                flat,
                expected,
            } => {
                out.word(token.get() as u64)?;
                out.word(flat.get() as u64)?;
                out.word(expected.get())
            }
            Self::CreateNest { license } => out.word(license.get() as u64),
            Self::DeleteNest {
                token,
                nest,
                expected,
            } => {
                out.word(token.get() as u64)?;
                out.word(nest.get() as u64)?;
                out.word(expected.get())
            }
            Self::Reserve {
                token,
                nest,
                expected,
                reservation,
            } => {
                out.word(token.get() as u64)?;
                out.word(nest.get() as u64)?;
                out.word(expected.get())?;
                out.bytes(&reservation.to_bytes())
            }
            Self::Release {
                token,
                nest,
                mapping,
                expected,
            } => {
                out.word(token.get() as u64)?;
                out.word(nest.get() as u64)?;
                out.word(mapping.get() as u64)?;
                out.word(expected.get())
            }
            Self::Split {
                start,
                expected,
                parent,
                maintenance,
            }
            | Self::Merge {
                start,
                expected,
                parent,
                maintenance,
            } => {
                out.bytes(&start.to_bytes())?;
                out.word(expected.get())?;
                out.word(parent.get() as u64)?;
                maintenance.write(out)
            }
            Self::Inspect(query) => query.write(out),
            Self::InspectResult { request, .. } => {
                out.word(3)?;
                out.word(request.get())
            }
            Self::Occupy { slot, expected } => write_slot(out, slot, expected),
            Self::Vacate {
                slot,
                expected,
                maintenance,
            } => {
                write_slot(out, slot, expected)?;
                maintenance.write(out)
            }
            Self::Block {
                start,
                count,
                recover,
            } => {
                out.bytes(&start.to_bytes())?;
                out.word(count as u64)?;
                out.word(u64::from(recover.is_some()))?;
                if let Some(block) = recover {
                    out.bytes(&block.to_bytes())?;
                }
                Some(())
            }
            Self::Allow { maintenance } => maintenance.write(out),
            Self::Push {
                local,
                local_version,
                remote,
                remote_version,
                maintenance,
            }
            | Self::Pull {
                local,
                local_version,
                remote,
                remote_version,
                maintenance,
            } => {
                write_slot(out, local, local_version)?;
                write_slot(out, remote, remote_version)?;
                write_maintenance(out, maintenance)
            }
            Self::Insert {
                target,
                expected,
                source,
            } => {
                write_memo(out, target, expected)?;
                source.write(out)
            }
            Self::Remove { memo, expected } | Self::Retract { memo, expected } => {
                write_memo(out, memo, expected)
            }
            Self::Publish {
                memo,
                expected,
                maintenance,
            } => {
                write_memo(out, memo, expected)?;
                write_maintenance(out, maintenance)
            }
            Self::Protect {
                memo,
                expected,
                access,
            } => {
                write_memo(out, memo, expected)?;
                out.word(access)
            }
        }
    }

    fn read(opcode: Opcode, input: &mut Reader<'_>) -> Option<Self> {
        Some(match opcode {
            Opcode::FaultRoute => Self::FaultRoute {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
                mapping: input.mapping()?,
                expected: MappingVersion::new(input.word()?)?,
                epoch: FaultEpoch::new(input.word()?)?,
                route: match input.word()? {
                    0 => None,
                    1 => Some(FaultRoute {
                        sign: input.token()?,
                        timeout_ns: input.word()?,
                    }),
                    _ => return None,
                },
            },
            Opcode::Create(Domain::Flat) => Self::CreateFlat {
                license: input.token()?,
                count: input.size()?,
                level: CellLevel::new(input.size()?)?,
            },
            Opcode::Delete(Domain::Flat) => Self::DeleteFlat {
                token: input.token()?,
                flat: FlatId::new(input.size()?)?,
                expected: FlatVersion::new(input.word()?)?,
            },
            Opcode::Create(Domain::Nest) => Self::CreateNest {
                license: input.token()?,
            },
            Opcode::Delete(Domain::Nest) => Self::DeleteNest {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
                expected: NestVersion::new(input.word()?)?,
            },
            Opcode::Reserve => Self::Reserve {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
                expected: NestVersion::new(input.word()?)?,
                reservation: ReserveRequest::from_bytes(input.bytes::<48>()?)?,
            },
            Opcode::Release => Self::Release {
                token: input.token()?,
                nest: NestId::new(input.size()?)?,
                mapping: input.mapping()?,
                expected: MappingVersion::new(input.word()?)?,
            },
            Opcode::Split | Opcode::Merge => {
                let start = SlotPosition::from_bytes(input.bytes::<24>()?)?;
                let expected = FlatVersion::new(input.word()?)?;
                let parent = CellLevel::new(input.size()?)?;
                let maintenance = Maintenance::read(input)?;
                if opcode == Opcode::Split {
                    Self::Split {
                        start,
                        expected,
                        parent,
                        maintenance,
                    }
                } else {
                    Self::Merge {
                        start,
                        expected,
                        parent,
                        maintenance,
                    }
                }
            }
            Opcode::Inspect(domain) => {
                let tag = input.word()?;
                if tag == 3 {
                    Self::InspectResult {
                        domain,
                        request: RequestId::new(input.word()?)?,
                    }
                } else {
                    Self::Inspect(Query::read(domain, tag, input)?)
                }
            }
            Opcode::Occupy => {
                let (slot, expected) = read_slot(input)?;
                Self::Occupy { slot, expected }
            }
            Opcode::Vacate => {
                let (slot, expected) = read_slot(input)?;
                Self::Vacate {
                    slot,
                    expected,
                    maintenance: Maintenance::read(input)?,
                }
            }
            Opcode::Block => {
                let start = SlotPosition::from_bytes(input.bytes::<24>()?)?;
                let count = input.size()?;
                let recover = match input.word()? {
                    0 => None,
                    1 => Some(BlockRef::from_bytes(input.bytes::<24>()?)?),
                    _ => return None,
                };
                Self::Block {
                    start,
                    count,
                    recover,
                }
            }
            Opcode::Allow => Self::Allow {
                maintenance: Maintenance::read(input)?,
            },
            Opcode::Push | Opcode::Pull => {
                let (local, local_version) = read_slot(input)?;
                let (remote, remote_version) = read_slot(input)?;
                let maintenance = read_maintenance(input)?;
                if opcode == Opcode::Push {
                    Self::Push {
                        local,
                        local_version,
                        remote,
                        remote_version,
                        maintenance,
                    }
                } else {
                    Self::Pull {
                        local,
                        local_version,
                        remote,
                        remote_version,
                        maintenance,
                    }
                }
            }
            Opcode::Insert => {
                let (target, expected) = read_memo(input)?;
                Self::Insert {
                    target,
                    expected,
                    source: Source::read(input)?,
                }
            }
            Opcode::Remove => {
                let (memo, expected) = read_memo(input)?;
                Self::Remove { memo, expected }
            }
            Opcode::Publish => {
                let (memo, expected) = read_memo(input)?;
                Self::Publish {
                    memo,
                    expected,
                    maintenance: read_maintenance(input)?,
                }
            }
            Opcode::Retract => {
                let (memo, expected) = read_memo(input)?;
                Self::Retract { memo, expected }
            }
            Opcode::Protect => {
                let (memo, expected) = read_memo(input)?;
                Self::Protect {
                    memo,
                    expected,
                    access: input.word()?,
                }
            }
        })
    }
}

/// Exactly 256 bytes: [version, domain, operation, request_id], action payload,
/// then reserved zero words. An inspection has request_id=0; every mutation has
/// a nonzero caller-chosen ID. No output pointers participate in request identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    id: Option<RequestId>,
    action: Action,
}

impl Request {
    pub const WIDTH: usize = 256;

    pub fn new(id: Option<RequestId>, action: Action) -> Option<Self> {
        let inspection = matches!(action, Action::InspectResult { .. } | Action::Inspect(_));
        (id.is_none() == inspection && action.valid()).then_some(Self { id, action })
    }

    pub const fn id(self) -> Option<RequestId> {
        self.id
    }
    pub const fn action(self) -> Action {
        self.action
    }

    pub fn to_bytes(self) -> [u8; Self::WIDTH] {
        let mut bytes = [0; Self::WIDTH];
        self.write(&mut Writer(&mut bytes))
            .expect("bounded memory request");
        bytes
    }

    fn write(self, out: &mut Writer<'_>) -> Option<()> {
        let opcode = self.action.opcode();
        for word in [
            VERSION,
            opcode.domain() as u64,
            opcode.slot(),
            self.id.map_or(0, RequestId::get),
        ] {
            out.word(word)?;
        }
        self.action.write(out)
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
        let id = RequestId::new(input.word()?);
        let action = Action::read(opcode, &mut input)?;
        input.finish()?;
        Self::new(id, action)
    }

    /// Entry must match the external trap number to the snapshotted descriptor.
    /// A structurally valid payload cannot select another operation/domain.
    pub fn from_call(number: u64, bytes: &[u8]) -> Option<Self> {
        let opcode = Opcode::from_call_number(number)?;
        let request = Self::from_bytes(bytes)?;
        (request.action.opcode() == opcode).then_some(request)
    }
}

fn write_slot(out: &mut Writer<'_>, position: SlotPosition, version: SlotVersion) -> Option<()> {
    out.bytes(&position.to_bytes())?;
    out.word(version.get())
}

fn read_slot(input: &mut Reader<'_>) -> Option<(SlotPosition, SlotVersion)> {
    Some((
        SlotPosition::from_bytes(input.bytes::<24>()?)?,
        SlotVersion::new(input.word()?)?,
    ))
}

fn write_memo(
    out: &mut Writer<'_>,
    position: MemoPosition,
    generation: MemoGeneration,
) -> Option<()> {
    out.bytes(&position.to_bytes())?;
    out.word(generation.get())
}

fn read_memo(input: &mut Reader<'_>) -> Option<(MemoPosition, MemoGeneration)> {
    Some((
        MemoPosition::from_bytes(input.bytes::<32>()?)?,
        MemoGeneration::new(input.word()?)?,
    ))
}

fn write_maintenance(out: &mut Writer<'_>, maintenance: Option<Maintenance>) -> Option<()> {
    out.word(u64::from(maintenance.is_some()))?;
    if let Some(maintenance) = maintenance {
        maintenance.write(out)?;
    }
    Some(())
}

fn read_maintenance(input: &mut Reader<'_>) -> Option<Option<Maintenance>> {
    Some(match input.word()? {
        0 => None,
        1 => Some(Maintenance::read(input)?),
        _ => return None,
    })
}
