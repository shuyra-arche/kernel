/// FQ evidence describes an access failure; it never proves DMA quiescence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fault {
    pub device: u32,
    pub cause: u16,
    pub transaction: u8,
    pub address: u64,
}

impl Fault {
    pub(super) fn decode(words: [u64; 4]) -> Self {
        Self {
            device: (words[0] >> 40) as u32,
            cause: (words[0] & 0xfff) as u16,
            transaction: ((words[0] >> 34) & 0x3f) as u8,
            address: words[2],
        }
    }
}
