//! One entry per qualified Retention, from preparation through exact settlement.
//! Unknown legacy participants cannot be included in a new fabric proof.
use super::RetainError;
use crate::metal::generic::memory::{self, Access, Endpoint, Member, Request, Scope};
use alloc::vec::Vec;
use core::num::NonZeroU64;

static NEXT: crate::basis::name::Serial = crate::basis::name::Serial::new(1, usize::MAX);

#[derive(Debug)]
pub(in super::super) struct Membership {
    entries: Vec<Member>,
    revision: u64,
}
impl Membership {
    pub(in super::super) const fn new() -> Self {
        Self {
            entries: Vec::new(),
            revision: 1,
        }
    }
    pub(super) fn prepare(
        &mut self,
        page: &super::super::Page,
        qualified: Option<(&memory::topology::Topology, Endpoint, Access)>,
    ) -> Result<Option<NonZeroU64>, RetainError> {
        let Some((topology, endpoint, access)) = qualified else {
            if !self.entries.is_empty() {
                return Err(RetainError::Borrowed);
            }
            self.revision = self.revision.saturating_add(1);
            return Ok(None);
        };
        if self.entries.len() >= memory::MAX_MEMBERS {
            return Err(RetainError::Exhausted);
        }
        let id = NEXT
            .allocate()
            .and_then(|v| NonZeroU64::new(v as u64))
            .ok_or(RetainError::Exhausted)?;
        let revision = self.revision.checked_add(1).ok_or(RetainError::Exhausted)?;
        let member = self
            .validate(page, topology, endpoint, access, id, revision)
            .map_err(|e| match e {
                memory::Error::Capacity => RetainError::Exhausted,
                _ => RetainError::Borrowed,
            })?;
        self.entries
            .try_reserve(1)
            .map_err(|_| RetainError::Exhausted)?;
        self.revision = revision;
        self.entries.push(member);
        Ok(Some(id))
    }
    pub(in super::super) fn check(
        &self,
        page: &super::super::Page,
        topology: &memory::topology::Topology,
        endpoint: Endpoint,
        access: Access,
    ) -> Result<(), memory::Error> {
        let id = NEXT
            .allocate()
            .and_then(|v| NonZeroU64::new(v as u64))
            .ok_or(memory::Error::Capacity)?;
        let revision = self
            .revision
            .checked_add(1)
            .ok_or(memory::Error::Capacity)?;
        self.validate(page, topology, endpoint, access, id, revision)
            .map(|_| ())
    }
    fn validate(
        &self,
        page: &super::super::Page,
        topology: &memory::topology::Topology,
        endpoint: Endpoint,
        access: Access,
        id: NonZeroU64,
        revision: u64,
    ) -> Result<Member, memory::Error> {
        if self.entries.len() >= memory::MAX_MEMBERS {
            return Err(memory::Error::Capacity);
        }
        // Legacy receipts remain counted by the same Page. Any unclassified
        // obligation makes a complete qualified proof impossible, even read-only.
        if page.reclaim.obligations() != self.entries.len() as u64 {
            return Err(memory::Error::Unsupported);
        }
        let candidate = Member {
            publication: id,
            generation: NonZeroU64::MIN,
            endpoint,
            access,
        };
        let mut peers = Vec::new();
        peers
            .try_reserve(self.entries.len())
            .map_err(|_| memory::Error::Capacity)?;
        for entry in &self.entries {
            peers.push(*entry);
        }
        let region = memory::Region::new(
            NonZeroU64::new(page.id().get() as u64).unwrap(),
            page.physical_address() as u64,
            page.size() as u64,
            memory::MemoryKind::Ram,
        )
        .ok_or(memory::Error::Unsupported)?;
        let request = Request {
            region,
            member: candidate,
            revision: NonZeroU64::new(revision).unwrap(),
            attempt: id,
            scope: Scope::Publication,
        };
        let group = memory::Group::new(request, &peers).map_err(|_| memory::Error::Unsupported)?;
        let proof = topology
            .compatible(group)
            .map_err(|_| memory::Error::Unsupported)?;
        if proof.request() != request {
            return Err(memory::Error::Unsupported);
        }
        Ok(candidate)
    }
    pub(super) fn remove(&mut self, id: Option<NonZeroU64>) {
        if let Some(id) = id {
            let index = self
                .entries
                .iter()
                .position(|e| e.publication == id)
                .expect("exact live retention");
            self.entries.swap_remove(index);
        }
        // Legacy cancellation/settlement also changes membership. Exhaustion
        // closes new qualified admission but never blocks existing cleanup.
        self.revision = self.revision.saturating_add(1);
    }
}
