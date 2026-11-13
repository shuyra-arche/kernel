mod allocation;
mod layout;
mod operation_slots;
mod operations;
mod typing;

mod cell;
mod pend;

mod unit;
mod version;

mod permission;

mod organization;

mod memory;

fn fetch_words<const N: usize>(bytes: &[u8]) -> Option<[usize; N]> {
    if bytes.len() != N * 8 {
        return None;
    }
    let mut words = [0; N];
    for (word, bytes) in words.iter_mut().zip(bytes.chunks_exact(8)) {
        *word = usize::try_from(u64::from_le_bytes(bytes.try_into().ok()?)).ok()?;
    }
    Some(words)
}
