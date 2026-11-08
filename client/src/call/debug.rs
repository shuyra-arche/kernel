//! Debug input and output.

use kernel_api::abi::call::debug::operations as op;
use kernel_api::{abi::call::debug::DebugFail, address::VirtAddr};

typed_calls! {
    fallible fn put(buf: VirtAddr, len: usize) -> Result<usize, erra::Error<DebugFail>> =
        op::Put { buf, len };
    fallible fn get(buf: VirtAddr, len: usize) -> Result<usize, erra::Error<DebugFail>> =
        op::Get { buf, len };
}
