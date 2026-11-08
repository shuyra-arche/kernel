//! Kernel-installed routes from an authorized MMIO range to an I/O domain.

use alloc::{boxed::Box, vec::Vec};

use crate::basis::{cell::page::PageId, flat::FlatRange, nest::authority::admission::Admission};
use crate::metal::generic::iommu::Port;
use crate::metal::sync::SpinLock;

use crate::basis::nest::PublicationError as Error;
use crate::metal::iommu::Backend;

pub(crate) struct Aperture {
    pub(crate) base: usize,
    pub(crate) did: u32,
    pub(crate) port: Box<dyn Port<PageId, Error = Error>>,
    pub(crate) source: FlatRange,
}

pub(crate) static APERTURES: SpinLock<Vec<Aperture>> = SpinLock::new(Vec::new());

pub(crate) fn bind(source: &FlatRange, owner: &Admission) -> Result<(u32, Backend), Error> {
    if owner.closed() {
        return Err(Error::Closed);
    }
    let mut ports = APERTURES.lock();
    let entry = ports
        .iter_mut()
        .find(|entry| entry.source.same_range(source))
        .ok_or(Error::InvalidRange)?;
    Ok((entry.did, entry.port.bind()?))
}
