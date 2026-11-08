use super::super::super::Fault;

pub(super) fn decode(words: [u64; 4]) -> Fault {
    Fault {
        device: (words[0] >> 40) as u32,
        cause: (words[0] & 0xfff) as u16,
        transaction: ((words[0] >> 34) & 0x3f) as u8,
        address: words[2],
    }
}
