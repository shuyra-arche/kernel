use kernel_api::{abi::call::pie::PieFail, boot::ledger::Entry};
use kernel_api::{boot::ledger::Header, boot::ledger::store_entries};

pub(crate) const BLOCK_BYTES: usize = crate::core::memory::PAGE_SIZE;
#[repr(C, align(4096))]
struct Block(::core::cell::UnsafeCell<[u8; BLOCK_BYTES]>);
unsafe impl Sync for Block {}
static BLOCK: Block = Block(::core::cell::UnsafeCell::new([0; BLOCK_BYTES]));

pub(crate) fn block() -> (usize, usize) {
    (::core::ptr::addr_of!(BLOCK) as usize, BLOCK_BYTES)
}
pub(crate) fn size(count: usize) -> Result<usize, PieFail> {
    let len = count
        .checked_mul(kernel_api::boot::ledger::ENTRY_LEN)
        .and_then(|n| n.checked_add(Header::LEN))
        .ok_or(PieFail::OoM)?;
    if len > BLOCK_BYTES {
        return Err(PieFail::OoM);
    }
    Ok(len)
}
pub(crate) fn write(entries: &[Entry]) -> Result<usize, PieFail> {
    if entries.iter().any(|entry| !entry.valid()) {
        return Err(PieFail::Denied);
    }
    let len = size(entries.len())?;
    let out = unsafe { &mut *BLOCK.0.get() };
    let end = store_entries(out, entries).ok_or(PieFail::Denied)?;
    if end != len {
        return Err(PieFail::Denied);
    }
    Ok(len)
}
