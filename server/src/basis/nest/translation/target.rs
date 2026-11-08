//! Hardware storage selected for a Nest. External storage contains no Page ownership.
use super::external::External;
use crate::metal::iommu::Backend;
use crate::metal::mmu::Table;

pub(in crate::basis::nest) enum Storage {
    Cpu(Table),
    External(External<Backend>),
}
impl Storage {
    pub(in crate::basis::nest) fn cpu(&self) -> Result<&Table, crate::basis::nest::NestError> {
        match self {
            Self::Cpu(table) => Ok(table),
            Self::External(_) => Err(crate::basis::nest::NestError::NoRegion),
        }
    }
    pub(in crate::basis::nest) fn cpu_mut(
        &mut self,
    ) -> Result<&mut Table, crate::basis::nest::NestError> {
        match self {
            Self::Cpu(table) => Ok(table),
            Self::External(_) => Err(crate::basis::nest::NestError::NoRegion),
        }
    }
}
