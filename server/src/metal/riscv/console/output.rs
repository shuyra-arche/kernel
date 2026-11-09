//! Bounded byte transfer: a virtual buffer need not be physically contiguous,
//! and SBI DBCN may accept only a prefix (including zero bytes).
use kernel_api::address::PAGE_SIZE;
pub(super) fn transfer(
    bytes: &[u8],
    mut physical: impl FnMut(usize, usize) -> Option<usize>,
    mut emit: impl FnMut(usize, usize) -> Option<usize>,
) -> Option<usize> {
    let mut written = 0;
    while written < bytes.len() {
        let address = bytes.as_ptr() as usize + written;
        let count = (PAGE_SIZE - address % PAGE_SIZE).min(bytes.len() - written);
        let accepted = physical(address, count).and_then(|pa| emit(pa, count));
        let Some(accepted) = accepted.filter(|n| *n <= count) else {
            return (written != 0).then_some(written);
        };
        written += accepted;
        if accepted < count {
            break;
        }
    }
    Some(written)
}
