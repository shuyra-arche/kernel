//! Immutable whole-fabric qualification supplied by trusted platform integration.
//! A fabric is a group guarantee, never a collection of CPU/device pair claims.
use super::*;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ram {
    pub base: u64,
    pub bytes: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    pub controller: u64,
    pub requester: u32,
}
/// All listed routes and all admitted Harts share these RAM intervals with the
/// same normal, cacheable, little-endian attributes. No bypass/ATS/PRI aliases.
pub struct Fabric {
    pub ram: Vec<Ram>,
    pub routes: Vec<Route>,
}
enum Instructions {
    Unsupported,
    LocalHart,
}
/// Kept immutable for the boot lifetime. Runtime endpoint epochs still belong
/// to the actual CPU domain and exclusive IOMMU binding, not these descriptions.
pub struct Topology {
    fabrics: Vec<Fabric>,
    instructions: Instructions,
}
impl Topology {
    /// # Safety
    /// Validate the actual machine, complete isolation groups and every alias.
    /// Each fabric independently supports ongoing coherent CPU/device and
    /// device/device reads/writes over every listed interval, with normal
    /// cacheable little-endian attributes and platform ordering. Separate
    /// pairwise coherence declarations do not satisfy this requirement.
    /// Routes cannot be rerouted, bypassed or configured outside trusted control
    /// during this boot. IOMMU CQ coherence does not establish payload coherence.
    /// Atomics beyond ordinary aligned accesses remain a service/platform contract.
    pub unsafe fn new(fabrics: Vec<Fabric>) -> Result<Self, Error> {
        if fabrics.is_empty() || fabrics.len() > MAX_MEMBERS {
            return Err(Error::Capacity);
        }
        for fabric in &fabrics {
            if fabric.ram.is_empty()
                || fabric.ram.len() > MAX_MEMBERS
                || fabric.routes.len() > MAX_MEMBERS
            {
                return Err(Error::Capacity);
            }
            for (i, ram) in fabric.ram.iter().enumerate() {
                if ram.bytes == 0
                    || !ram.base.is_multiple_of(4096)
                    || !ram.bytes.is_multiple_of(4096)
                    || ram.base.checked_add(ram.bytes).is_none()
                    || fabric.ram[..i]
                        .iter()
                        .any(|r| r.base < ram.base + ram.bytes && ram.base < r.base + r.bytes)
                {
                    return Err(Error::Invalid);
                }
            }
            for (i, route) in fabric.routes.iter().enumerate() {
                if route.controller == 0
                    || !route.controller.is_multiple_of(4096)
                    || fabric.routes[..i].contains(route)
                {
                    return Err(Error::Invalid);
                }
            }
        }
        Ok(Self {
            fabrics,
            instructions: Instructions::Unsupported,
        })
    }
    /// Qualify instruction fetch on the single admitted CPU Hart.
    ///
    /// # Safety
    /// All instruction fetches for these RAM intervals occur on that Hart,
    /// whose FENCE.I synchronizes prior visible writes before execution. No
    /// device fetch, bypass or executable alias exists outside this model.
    /// The installed backend must reject SMP and execute FENCE.I before
    /// completing executable Publish/Protect. This does not relax group W/X.
    pub unsafe fn with_local_instructions(mut self) -> Self {
        self.instructions = Instructions::LocalHart;
        self
    }
    pub fn allows_local_instructions(&self) -> bool {
        matches!(self.instructions, Instructions::LocalHart)
    }
    pub(crate) fn regions(&self) -> impl Iterator<Item = Ram> + '_ {
        self.fabrics.iter().flat_map(|f| f.ram.iter().copied())
    }
    /// No hardware effects and no allocation. One fabric must cover the complete
    /// group and the entire interval. Never union two partial fabric guarantees.
    pub fn compatible(&self, group: Group<'_>) -> Result<Evidence<Compatibility>, Error> {
        let request = group.request();
        let executable = request.member.access.executable()
            || group.existing().iter().any(|m| m.access.executable());
        if executable
            && (!self.allows_local_instructions()
                || group
                    .existing()
                    .iter()
                    .chain(core::iter::once(&request.member))
                    .any(|m| matches!(m.endpoint, Endpoint::External { .. })))
        {
            return Err(Error::Unsupported);
        }
        let region = request.region;
        let covered = self.fabrics.iter().any(|fabric| {
            fabric.ram.iter().any(|r| {
                r.base <= region.physical()
                    && region.physical() + region.bytes() <= r.base + r.bytes
            }) && group
                .existing()
                .iter()
                .chain(core::iter::once(&request.member))
                .all(|m| match m.endpoint {
                    Endpoint::Cpu { .. } => true,
                    Endpoint::External {
                        controller,
                        requester,
                        ..
                    } => {
                        !m.access.executable()
                            && fabric.routes.contains(&Route {
                                controller,
                                requester,
                            })
                    }
                })
        });
        if !covered {
            return Err(Error::Unsupported);
        }
        // SAFETY: constructor established an immutable complete-fabric guarantee;
        // Group validates RAM, bounds, identities and cross-member W/X exclusion.
        Ok(unsafe { Evidence::attest(request) })
    }
}
