//! Next-memory proof contract and immutable complete-fabric qualification.
//! This module installs no provider. Numeric request fields are not evidence;
//! only trusted integration may establish physical facts. Runtime activation
//! remains coordinated separately from these Rust interfaces.

use core::{marker::PhantomData, num::NonZeroU64};

pub mod topology;

pub const MAX_MEMBERS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryKind {
    Ram,
    Mmio,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    page: NonZeroU64,
    physical: u64,
    bytes: u64,
    kind: MemoryKind,
}
impl Region {
    pub fn new(page: NonZeroU64, physical: u64, bytes: u64, kind: MemoryKind) -> Option<Self> {
        (bytes > 0
            && bytes.is_multiple_of(4096)
            && physical.is_multiple_of(4096)
            && physical.checked_add(bytes).is_some())
        .then_some(Self {
            page,
            physical,
            bytes,
            kind,
        })
    }
    pub const fn page(self) -> NonZeroU64 {
        self.page
    }
    pub const fn physical(self) -> u64 {
        self.physical
    }
    pub const fn bytes(self) -> u64 {
        self.bytes
    }
    pub const fn kind(self) -> MemoryKind {
        self.kind
    }
}

/// An immutable trusted binding epoch, not a caller's Task/Team identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endpoint {
    Cpu {
        nest: NonZeroU64,
        leases: NonZeroU64,
    },
    External {
        controller: u64,
        requester: u32,
        binding: NonZeroU64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Access(u8);
impl Access {
    /// Next-memory access bits; this first contract forbids writable executable RAM.
    pub fn new(bits: u8) -> Option<Self> {
        matches!(bits, 2 | 6 | 8 | 10).then_some(Self(bits))
    }
    pub const fn bits(self) -> u8 {
        self.0
    }
    pub const fn writable(self) -> bool {
        self.0 & 4 != 0
    }
    pub const fn executable(self) -> bool {
        self.0 & 8 != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member {
    pub publication: NonZeroU64,
    pub generation: NonZeroU64,
    pub endpoint: Endpoint,
    pub access: Access,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Publication,
    CpuAddressSpace,
    CpuGlobal,
    ExternalBinding,
}

/// Exact Page, publication, endpoint epoch, membership revision and attempt.
/// The issuing owner must never reuse these identities or reset their counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub region: Region,
    pub member: Member,
    pub revision: NonZeroU64,
    pub attempt: NonZeroU64,
    pub scope: Scope,
}
impl Request {
    pub fn valid(self) -> bool {
        matches!(
            (self.member.endpoint, self.scope),
            (_, Scope::Publication)
                | (
                    Endpoint::Cpu { .. },
                    Scope::CpuAddressSpace | Scope::CpuGlobal
                )
                | (Endpoint::External { .. }, Scope::ExternalBinding)
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Unsupported,
    Capacity,
    RequiredScope(Scope),
    Fault,
    TimedOut,
    Stale,
}

/// Complete Page-wide membership snapshot, including prepared, publishing,
/// retiring and unresolved-failure responsibilities. The Page owner supplies it
/// under its admission boundary; the backend must assess the whole set at once.
pub struct Group<'a> {
    request: Request,
    existing: &'a [Member],
}
impl<'a> Group<'a> {
    pub fn new(request: Request, existing: &'a [Member]) -> Result<Self, Error> {
        if !request.valid() {
            return Err(Error::Invalid);
        }
        if request.region.kind != MemoryKind::Ram {
            return Err(Error::Unsupported);
        }
        if existing.len() >= MAX_MEMBERS {
            return Err(Error::Capacity);
        }
        let mut write = request.member.access.writable();
        let mut execute = request.member.access.executable();
        for (i, member) in existing.iter().enumerate() {
            if member.publication == request.member.publication
                || existing[..i]
                    .iter()
                    .any(|m| m.publication == member.publication)
            {
                return Err(Error::Invalid);
            }
            write |= member.access.writable();
            execute |= member.access.executable();
        }
        if write && execute {
            return Err(Error::Unsupported);
        }
        Ok(Self { request, existing })
    }
    pub const fn request(&self) -> Request {
        self.request
    }
    pub const fn existing(&self) -> &'a [Member] {
        self.existing
    }
}

pub enum Compatibility {}
pub enum DataVisible {}
pub enum InstructionsVisible {}
pub enum TranslationsRetired {}
pub enum AccessesDrained {}
pub enum AuxiliaryQuiescent {}

/// Single-use evidence. Not Clone/Copy, not serialized, and dropping it never
/// settles a resource. An equality check alone cannot construct this value.
#[must_use]
pub struct Evidence<K> {
    request: Request,
    kind: PhantomData<K>,
}
impl<K> Evidence<K> {
    /// # Safety
    /// Trusted provider only: establish the complete obligation for K, this exact
    /// request and all its participants/aliases. Never attest merely because a
    /// token was echoed, an IRQ arrived, a deadline expired or a driver replied.
    pub unsafe fn attest(request: Request) -> Self {
        Self {
            request,
            kind: PhantomData,
        }
    }
    pub const fn request(&self) -> Request {
        self.request
    }
}

/// A checked admission bundle; still requires live grants and the same Page
/// membership revision at the commit boundary. It is not permission to publish.
#[must_use]
pub struct Visible(Request);
impl Visible {
    pub fn confirms(self, request: Request) -> bool {
        self.0 == request
    }
}

pub fn visible(
    request: Request,
    compatible: Evidence<Compatibility>,
    data: Evidence<DataVisible>,
    instructions: Option<Evidence<InstructionsVisible>>,
) -> Result<Visible, Error> {
    if !request.valid() {
        return Err(Error::Invalid);
    }
    if request.region.kind != MemoryKind::Ram {
        return Err(Error::Unsupported);
    }
    if compatible.request != request
        || data.request != request
        || instructions
            .as_ref()
            .is_some_and(|proof| proof.request != request)
        || (request.member.access.executable() && instructions.is_none())
    {
        return Err(Error::Stale);
    }
    Ok(Visible(request))
}

/// Both translation retirement and completion of prior accesses are necessary.
/// This settles only one publication, never the Page's allocation or its peers.
#[must_use]
pub struct Retired(Request);
impl Retired {
    pub fn confirms(self, request: Request) -> bool {
        self.0 == request
    }
}

pub fn retired(
    request: Request,
    translations: Evidence<TranslationsRetired>,
    accesses: Evidence<AccessesDrained>,
) -> Result<Retired, Error> {
    if !request.valid() {
        return Err(Error::Invalid);
    }
    if request.region.kind != MemoryKind::Ram {
        return Err(Error::Unsupported);
    }
    if translations.request != request || accesses.request != request {
        return Err(Error::Stale);
    }
    Ok(Retired(request))
}

/// Inaccessibility of one detached table/queue/context allocation. The allocator
/// still owns exclusivity and freeing; this is not a payload retirement receipt.
#[must_use]
pub struct Quiescent(Request);
impl Quiescent {
    pub fn confirms(self, request: Request) -> bool {
        self.0 == request
    }
}

pub fn auxiliary(
    request: Request,
    proof: Evidence<AuxiliaryQuiescent>,
) -> Result<Quiescent, Error> {
    if !request.valid() || request.scope == Scope::Publication {
        return Err(Error::Invalid);
    }
    if request.region.kind != MemoryKind::Ram {
        return Err(Error::Unsupported);
    }
    if proof.request != request {
        return Err(Error::Stale);
    }
    Ok(Quiescent(request))
}

/// Trusted boot Rust interface, not a syscall or a userspace-installed callback.
/// Each method is a bounded progress step; None/error retains every obligation.
/// There is deliberately no default success and no coherence boolean.
///
/// # Safety
/// Compatibility covers ordinary RAM and the entire supplied group, actual
/// requester topology, cache/memory attributes, atomic/ordering limitations and
/// every relevant alias. Pairwise CPU/device relations do not prove group safety.
/// Data visibility precedes exposure; instruction visibility additionally covers
/// all executing Harts. CPU translation proofs cover the full lease epoch and
/// local/remote invalidations; drain proofs separately cover prior accesses.
/// External proofs require the exact controller/requester/binding, translation
/// invalidation AND independent platform draining of requests outside the IOMMU.
/// MMIO, unknown relations, noncoherent sustained sharing, ATS/PRI and bypass are
/// unsupported in the first activation. A coarser boundary returns RequiredScope;
/// it must not silently withdraw peer mappings. Recheck authority before widening.
/// Failures after possible exposure retain payloads and auxiliary tables/queues/
/// contexts. These methods never free them. Binding retirement must also prevent
/// delayed old submissions; device reassignment needs separate trusted isolation.
/// Auxiliary evidence covers the exact detached allocation, no reachable old
/// walker/context/queue and no future access by the old binding/CPU lease epoch.
pub unsafe trait Backend: Send {
    fn auxiliary_quiescent(
        &mut self,
        _request: Request,
    ) -> Result<Option<Evidence<AuxiliaryQuiescent>>, Error> {
        Err(Error::Unsupported)
    }
    fn compatible(&mut self, _group: Group<'_>) -> Result<Option<Evidence<Compatibility>>, Error> {
        Err(Error::Unsupported)
    }
    fn data_visible(&mut self, _request: Request) -> Result<Option<Evidence<DataVisible>>, Error> {
        Err(Error::Unsupported)
    }
    fn instructions_visible(
        &mut self,
        _request: Request,
    ) -> Result<Option<Evidence<InstructionsVisible>>, Error> {
        Err(Error::Unsupported)
    }
    fn translations_retired(
        &mut self,
        _request: Request,
    ) -> Result<Option<Evidence<TranslationsRetired>>, Error> {
        Err(Error::Unsupported)
    }
    fn accesses_drained(
        &mut self,
        _request: Request,
    ) -> Result<Option<Evidence<AccessesDrained>>, Error> {
        Err(Error::Unsupported)
    }
}

/// Safe fallback until a platform supplies and validates the complete contract.
pub struct Unsupported;
// SAFETY: every method refuses; no evidence or hardware effects are produced.
unsafe impl Backend for Unsupported {}
