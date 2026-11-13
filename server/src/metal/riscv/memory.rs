//! Boot-selected normal-RAM fabric facts. Installation alone activates no ABI.
use crate::metal::generic::memory::{Error, topology::Topology};
use crate::metal::sync::OnceLock;
static TOPOLOGY: OnceLock<Topology> = OnceLock::new();

pub(crate) fn install(topology: Topology) -> Result<(), Error> {
    // The first concrete drain path is local: remote SFENCE acknowledgements
    // alone do not establish completion of other Harts' prior data accesses.
    if crate::metal::hart::hart_count() != 1 {
        return Err(Error::Unsupported);
    }
    for region in topology.regions() {
        let base = usize::try_from(region.base).map_err(|_| Error::Invalid)?;
        let bytes = usize::try_from(region.bytes).map_err(|_| Error::Invalid)?;
        if !crate::entry::boot::facts::contains_identity(base, bytes) {
            return Err(Error::Unsupported);
        }
    }
    TOPOLOGY.set(topology).map_err(|_| Error::Stale)
}
pub(crate) fn topology() -> Option<&'static Topology> {
    TOPOLOGY.get()
}
/// Order prior payload writes before publishing translations. Coherent payload
/// visibility is a separately admitted platform fact; this is not a drain proof.
pub(crate) fn data_visible() {
    unsafe { core::arch::asm!("fence rw, rw", options(nostack)) };
}

/// Called after exact translation invalidation. On the admitted single-Hart
/// path this orders/completes earlier explicit loads and stores before reuse.
/// Unsupported remote drain paths never manufacture a completion receipt.
pub(crate) fn cpu_drained() -> Result<(), Error> {
    if topology().is_some() {
        if crate::metal::hart::hart_count() != 1 {
            return Err(Error::Unsupported);
        }
        data_visible();
    }
    Ok(())
}
