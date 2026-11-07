#[derive(Debug, Clone, Copy)]
pub enum ResourceOperation {
    InitializeTraps,
    InitializeIommu,
    Devices,
    Traps,
    Freeze,
    LedgerSize,
    Pend,
    WriteLedger,
}

impl ResourceOperation {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::InitializeTraps => "initialize trap resources",
            Self::InitializeIommu => "initialize IOMMU command engine",
            Self::Devices => "register device resources",
            Self::Traps => "register trap resources",
            Self::Freeze => "freeze resources",
            Self::LedgerSize => "size resource ledger",
            Self::Pend => "authority bootstrap resources",
            Self::WriteLedger => "write resource ledger",
        }
    }
}
