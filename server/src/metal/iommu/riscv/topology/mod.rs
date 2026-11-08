//! Validated firmware routing, not a authority to a Unit. The trusted binding path
//! must still establish device ownership, immutable topology, borrowing and
//! platform quiescence. No register is read or written while building this plan.

use crate::metal::board::tree::{
    self, Identities, enabled, mmio, property, scalar, word as number,
};
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteError {
    Malformed,
    Unsupported,
    Ambiguous,
    Unmapped,
    Memory,
}

#[derive(Clone, Copy)]
struct Controller {
    phandle: u32,
    base: usize,
}

struct Window {
    active: bool,
    host: usize,
    first: u32,
    end: u32,
    controller: usize,
    device: u32,
}

/// Values can only be obtained from a validated, unambiguous map.
pub(crate) struct Route {
    pub(super) controller: usize,
    pub(super) device: u32,
}

impl Route {
    pub(crate) fn device(&self) -> u32 {
        self.device
    }
}

pub(crate) struct Topology {
    controllers: Vec<Controller>,
    windows: Vec<Window>,
    hosts: Vec<(usize, usize)>,
}

impl Topology {
    pub(crate) fn discover(fdt: &fdt::Fdt) -> Result<Self, RouteError> {
        let mut result = Self {
            controllers: Vec::new(),
            windows: Vec::new(),
            hosts: Vec::new(),
        };
        let identities = Identities::discover(fdt)?;
        for node in fdt.all_nodes() {
            let platform = compatible(node, "riscv,iommu");
            if !platform && property(node, "#iommu-cells")?.is_none() {
                continue;
            }
            if !platform || compatible(node, "riscv,pci-iommu") {
                return Err(RouteError::Unsupported);
            }
            if !enabled(fdt, node)? {
                continue;
            }
            if scalar(node, "#iommu-cells")? != Some(1) {
                return Err(RouteError::Unsupported);
            }
            let phandle = identities.id(node).ok_or(RouteError::Malformed)?;
            let region = mmio::single(fdt, node)?;
            let (base, size) = (region.base, region.size);
            if base == 0 || !base.is_multiple_of(4096) || size != 4096 {
                return Err(RouteError::Unsupported);
            }
            if result.controllers.iter().any(|c| c.base == base) {
                return Err(RouteError::Ambiguous);
            }
            result
                .controllers
                .try_reserve(1)
                .map_err(|_| RouteError::Memory)?;
            result.controllers.push(Controller { phandle, base });
        }
        for node in fdt.all_nodes() {
            let active = enabled(fdt, node)?;
            if property(node, "iommu-map")?.is_none() {
                // No implicit identity/bypass fallback, including mask-only hosts.
                if property(node, "iommu-map-mask")?.is_some()
                    || property(node, "iommus")?.is_some()
                {
                    return Err(RouteError::Unsupported);
                }
                continue;
            }
            if property(node, "device_type")? != Some(b"pci\0")
                || !compatible(node, "pci-host-ecam-generic")
                || property(node, "iommus")?.is_some()
            {
                return Err(RouteError::Unsupported);
            }
            // Masking out RID bits aliases otherwise independent requesters.
            // Supporting those requires isolation groups, not individual authorities.
            if scalar(node, "iommu-map-mask")?.is_some_and(|mask| mask & 0xffff != 0xffff) {
                return Err(RouteError::Unsupported);
            }
            let region = mmio::single(fdt, node)?;
            let (host, size) = (region.base, region.size);
            if host == 0 || !host.is_multiple_of(1 << 20) || !size.is_multiple_of(1 << 20) {
                return Err(RouteError::Unsupported);
            }
            let bus = property(node, "bus-range")?.ok_or(RouteError::Malformed)?;
            if bus.len() != 8 {
                return Err(RouteError::Malformed);
            }
            let first = number(&bus[..4])?;
            let last = number(&bus[4..])?;
            if first > last || last > 255 || size < ((last - first + 1) as usize) << 20 {
                return Err(RouteError::Malformed);
            }
            let edge = host.checked_add(size).ok_or(RouteError::Malformed)?;
            if result.hosts.iter().any(|&(a, b)| host < b && a < edge) {
                return Err(RouteError::Ambiguous);
            }
            result
                .hosts
                .try_reserve(1)
                .map_err(|_| RouteError::Memory)?;
            result.hosts.push((host, edge));
            let map = property(node, "iommu-map")?.expect("map present");
            if map.is_empty() || !map.len().is_multiple_of(16) {
                return Err(RouteError::Malformed);
            }
            for tuple in map.chunks_exact(16) {
                let rid = number(&tuple[..4])?;
                let phandle = number(&tuple[4..8])?;
                let device = number(&tuple[8..12])?;
                let length = number(&tuple[12..])?;
                let controller = result
                    .controllers
                    .iter()
                    .find(|c| c.phandle == phandle)
                    .ok_or(RouteError::Unmapped)?
                    .base;
                let end = rid.checked_add(length).ok_or(RouteError::Malformed)?;
                let device_end = device.checked_add(length).ok_or(RouteError::Malformed)?;
                if rid > 0xffff || end > 0x10000 || device >= 1 << 24 || device_end > 1 << 24 {
                    return Err(RouteError::Malformed);
                }
                // QEMU 10.0 emits an empty interval before its effective mapping.
                // It authorities nothing; still validate its target and arithmetic.
                if length == 0 {
                    continue;
                }
                if rid < first << 8 || end > (last + 1) << 8 {
                    return Err(RouteError::Malformed);
                }
                for old in &result.windows {
                    if (old.host == host && rid < old.end && old.first < end)
                        || (old.controller == controller
                            && device < old.device + old.end - old.first
                            && old.device < device_end)
                    {
                        return Err(RouteError::Ambiguous);
                    }
                }
                result
                    .windows
                    .try_reserve(1)
                    .map_err(|_| RouteError::Memory)?;
                result.windows.push(Window {
                    active,
                    host,
                    first: rid,
                    end,
                    controller,
                    device,
                });
            }
        }
        Ok(result)
    }

    pub(crate) fn controllers(&self) -> impl Iterator<Item = usize> + '_ {
        self.controllers.iter().map(|controller| controller.base)
    }

    /// Whole ECAM windows stay in Kernel custody so a Unit cannot renumber
    /// requesters, enable ATS or mutate the configuration behind this snapshot.
    pub(crate) fn configurations(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.hosts.iter().map(|&(base, edge)| (base, edge - base))
    }

    pub(crate) fn resolve(&self, host: usize, requester: u16) -> Result<Route, RouteError> {
        let rid = u32::from(requester);
        let w = self
            .windows
            .iter()
            .find(|w| w.active && w.host == host && w.first <= rid && rid < w.end)
            .ok_or(RouteError::Unmapped)?;
        Ok(Route {
            controller: w.controller,
            device: w.device + rid - w.first,
        })
    }
}

fn compatible(node: fdt::node::FdtNode, value: &str) -> bool {
    node.compatible()
        .is_some_and(|c| c.all().any(|s| s == value))
}

impl From<tree::Error> for RouteError {
    fn from(error: tree::Error) -> Self {
        match error {
            tree::Error::Description => Self::Malformed,
            tree::Error::OutOfMemory => Self::Memory,
        }
    }
}
