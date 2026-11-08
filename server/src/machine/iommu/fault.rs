/// FQ evidence describes an access failure; it never proves DMA quiescence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fault {
    pub device: u32,
    pub cause: u16,
    pub transaction: u8,
    pub address: u64,
}
