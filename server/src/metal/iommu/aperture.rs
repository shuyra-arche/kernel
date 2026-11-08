//! Kernel-installed routes from an authorized MMIO range to an I/O domain.

use alloc::{boxed::Box, vec::Vec};

use crate::basis::{
    cell::{page::PageId, projection::Admission},
    flat::FlatRange,
};
use crate::metal::generic::iommu::Port;
use crate::metal::sync::SpinLock;

use super::retirement::{Completion, DomainId};
use super::{Error, runtime};

pub(crate) struct Aperture {
    pub(crate) base: usize,
    pub(crate) did: u32,
    pub(crate) port: Box<dyn Port<PageId, Error = Error>>,
    pub(crate) source: FlatRange,
}

pub(crate) static APERTURES: SpinLock<Vec<Aperture>> = SpinLock::new(Vec::new());

pub(crate) struct Binding {
    pub(crate) did: u32,
    pub(crate) domain: DomainId,
    pub(crate) completion: Completion,
}

pub(crate) fn bind(source: &FlatRange, owner: &Admission) -> Result<Binding, Error> {
    if owner.closed() {
        return Err(Error::Closed);
    }
    let mut ports = APERTURES.lock();
    let entry = ports
        .iter_mut()
        .find(|entry| entry.source.same_range(source))
        .ok_or(Error::InvalidRange)?;
    let backend = entry.port.bind()?;
    let domain = runtime::install(owner, backend).map_err(|(error, _)| error)?;
    let completion = runtime::completion(domain).expect("installed domain");
    Ok(Binding {
        did: entry.did,
        domain,
        completion,
    })
}
