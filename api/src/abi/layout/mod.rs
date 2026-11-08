//! Concrete byte descriptors passed through caller-owned cell.
mod cell;
mod pend;

/// The RV64 ABI's byte width, independent of the build host's pointer width.
pub(crate) const WORD_BYTES: usize = 8;

pub(crate) fn store_words<const N: usize>(words: [usize; N], out: &mut [u8]) {
    assert_eq!(out.len(), N * WORD_BYTES);
    for (word, bytes) in words.into_iter().zip(out.chunks_exact_mut(WORD_BYTES)) {
        bytes.copy_from_slice(&(word as u64).to_le_bytes());
    }
}

pub(crate) fn fetch_words<const N: usize>(bytes: &[u8]) -> Option<[usize; N]> {
    if bytes.len() != N * WORD_BYTES {
        return None;
    }
    let mut words = [0; N];
    for (word, bytes) in words.iter_mut().zip(bytes.chunks_exact(WORD_BYTES)) {
        *word = usize::try_from(u64::from_le_bytes(bytes.try_into().ok()?)).ok()?;
    }
    Some(words)
}

mod unit;

mod object;
