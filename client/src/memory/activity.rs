//! Resolve the ordinary Pend pair produced by Share.Derive. This adds no trap
//! number, permission cache or opaque subscription identity.
use crate::call::pend;
use kernel_api::{
    abi::call::pend::{PendFail, PendResult},
    address::VirtAddr,
    model::{
        identity::PendId,
        memory::activity::{self as protocol, Pair},
        pend::{PendInfo, PendStatus},
    },
};

/// `view` is the receiver-local result of existing Share.Derive. Call this in
/// that receiving Task. Busy retains the same View; do not derive another one.
pub fn pair(view: PendId) -> PendResult<Pair> {
    let mut bytes = [0; PendInfo::SIZE];
    pend::inspect(view, VirtAddr::new(bytes.as_mut_ptr() as usize))?;
    let view = PendInfo::from_bytes(&bytes)
        .ok_or_else(|| erra::Error::new("activity View", PendFail::Invalid))?;
    if view.status != PendStatus::Active {
        return Err(erra::Error::new("activity View", PendFail::Dead));
    }
    let mut after = PendId::from_bytes(&0u64.to_le_bytes()).expect("collect sentinel");
    let mut batch = [0; PendInfo::SIZE * 8];
    // Receiver admission is 1024 entries. Concurrent traffic cannot turn this
    // lookup into an unbounded traversal; retry the same View if necessary.
    for _ in 0..128 {
        let count = pend::collect(after, VirtAddr::new(batch.as_mut_ptr() as usize), 8)?;
        if count > 8 {
            return Err(erra::Error::new("activity pair", PendFail::Invalid));
        }
        for bytes in batch[..count * PendInfo::SIZE].chunks_exact(PendInfo::SIZE) {
            let info = PendInfo::from_bytes(bytes)
                .ok_or_else(|| erra::Error::new("activity Sign", PendFail::Invalid))?;
            after = info.token;
            if let Some(pair) = protocol::pair(info, view) {
                return Ok(pair);
            }
        }
        if count < 8 {
            break;
        }
    }
    Err(erra::Error::new("activity pair", PendFail::Busy))
}
