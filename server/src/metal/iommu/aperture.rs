//! Kernel-installed routes from an authorized MMIO range to an I/O domain.

use alloc::{boxed::Box, vec::Vec};

use crate::basis::{cell::projection::Admission, flat::FlatRange};
use crate::metal::sync::SpinLock;

use super::retirement::{Completion, DomainId};
use super::{Error, runtime};

/// A trusted port bound to one requester. The provider owns platform effects
/// such as admission and blocking future requests.
///
/// # Safety
/// A bound backend must cover every request issued through the port. After an
/// irreversible binding attempt, the port must reject a second bind until the
/// implementation can prove reauthorization safe.
pub(crate) unsafe trait Port: Send {
    fn bind(&mut self) -> Result<runtime::Backend, Error>;

    /// # Safety
    /// The matching retained I/O space must be active and authorized.
    unsafe fn activate(&mut self) -> Result<(), Error>;

    fn stop(&mut self) -> Result<(), Error>;
}

pub(crate) struct Aperture {
    pub(crate) base: usize,
    pub(crate) did: u32,
    pub(crate) port: Box<dyn Port>,
    pub(crate) source: FlatRange,
}

pub(crate) static APERTURES: SpinLock<Vec<Aperture>> = SpinLock::new(Vec::new());

pub(crate) struct Binding {
    pub(crate) port: usize,
    pub(crate) did: u32,
    pub(crate) domain: DomainId,
    pub(crate) completion: Completion,
}

pub(crate) fn bind(source: &FlatRange, owner: &Admission) -> Result<Binding, Error> {
    if owner.closed() {
        return Err(Error::Closed);
    }
    let mut ports = APERTURES.lock();
    let (port, entry) = ports
        .iter_mut()
        .enumerate()
        .find(|(_, entry)| entry.source.same_range(source))
        .ok_or(Error::InvalidRange)?;
    let backend = entry.port.bind()?;
    let domain = runtime::install(owner, backend).map_err(|(error, _)| error)?;
    let completion = runtime::completion(domain).expect("installed domain");
    Ok(Binding {
        port,
        did: entry.did,
        domain,
        completion,
    })
}

/// # Safety
/// The caller verified that the exact domain bound to this port is active.
pub(crate) unsafe fn activate(port: usize) -> Result<(), Error> {
    let mut ports = APERTURES.lock();
    let entry = ports.get_mut(port).ok_or(Error::InvalidRange)?;
    unsafe { entry.port.activate() }
}

pub(crate) fn stop(port: usize) -> Result<(), Error> {
    APERTURES
        .lock()
        .get_mut(port)
        .ok_or(Error::InvalidRange)?
        .port
        .stop()
}
