//! Normalized RISC-V CPU admission; no firmware strings or topology pointers.
mod id;
pub use id::{FirmwareId, HartId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Paging {
    Sv39,
    Sv48,
    Sv57,
}
#[derive(Debug, Clone, Copy)]
pub struct Cpu {
    pub id: FirmwareId,
    pub paging: Paging,
    pub floating: bool,
    pub vector: bool,
    pub sstc: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HartError {
    Capacity,
    Identity,
    Boot,
}
#[derive(Debug)]
pub struct Harts {
    cpus: [Cpu; 256],
    count: usize,
    hertz: usize,
    paging: Paging,
}
impl Harts {
    pub fn new(cpus: &[Cpu], boot: FirmwareId, hertz: usize) -> Result<Self, HartError> {
        if cpus.is_empty() || cpus.len() > 256 || hertz == 0 {
            return Err(HartError::Capacity);
        }
        if cpus[0].id != boot {
            return Err(HartError::Boot);
        }
        for (i, cpu) in cpus.iter().enumerate() {
            if cpu.id.get() == usize::MAX || cpus[..i].iter().any(|old| old.id == cpu.id) {
                return Err(HartError::Identity);
            }
        }
        let mut result = Self {
            cpus: [cpus[0]; 256],
            count: cpus.len(),
            hertz,
            paging: Paging::Sv57,
        };
        result.cpus[..cpus.len()].copy_from_slice(cpus);
        for cpu in cpus {
            result.paging = result.paging.min(cpu.paging);
        }
        Ok(result)
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn hertz(&self) -> usize {
        self.hertz
    }
    pub fn paging(&self) -> Paging {
        self.paging
    }
    pub fn firmware(&self, slot: HartId) -> FirmwareId {
        self.cpu(slot).id
    }
    pub fn floating(&self, slot: HartId) -> bool {
        self.cpu(slot).floating
    }
    pub fn vector(&self, slot: HartId) -> bool {
        self.cpu(slot).vector
    }
    pub fn sstc(&self, slot: HartId) -> bool {
        self.cpu(slot).sstc
    }
    fn cpu(&self, slot: HartId) -> &Cpu {
        &self.cpus[..self.count][slot.get()]
    }
}

impl core::fmt::Display for HartError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::Capacity => "admission capacity exceeded",
            Self::Identity => "duplicate or reserved CPU identity",
            Self::Boot => "slot zero is not the boot hart",
        })
    }
}
impl core::error::Error for HartError {}
