#![no_std]
#![no_main]

use kernel_std::env::{PieToken, TeamId, ViewControl, Wait};
use kernel_std_rt::entry;

struct DemoMessage;

impl kernel_std::wire::Message for DemoMessage {
    type In = u8;
    type Buf = [u8; 1];
    const EMPTY: Self::Buf = [0];

    fn store(&self, out: &mut [u8]) -> Option<usize> {
        *out.first_mut()? = 1;
        Some(1)
    }

    fn fetch(bytes: &[u8]) -> Option<Self::In> {
        (bytes.len() == 1).then_some(bytes[0])
    }
}

fn dummy_entry() -> ! {
    loop {}
}

#[inline(never)]
fn exercise_mechanisms() {
    let _ = core::hint::black_box(kernel_std::execution::memory::attach_alloc(
        4096,
        ViewControl::Caller,
    ));
    let _ = core::hint::black_box(kernel_std::execution::unit::spawn(
        TeamId::new(0),
        dummy_entry as *const () as usize,
        &[],
        0,
    ));
    let _ = core::hint::black_box(kernel_std::resource::raw::inspect(PieToken::NONE));
    let _ = core::hint::black_box(kernel_std::ipc::time::deadline(Wait::POLL));
    let _ = core::hint::black_box(kernel_std::ipc::hand::Sender::<DemoMessage>::new());
    let _ = core::hint::black_box(kernel_std::ipc::hand::Receiver::<DemoMessage>::from_raw(
        PieToken::NONE,
    ));
}

fn main() -> Result<(), kernel_std::env::Reason> {
    exercise_mechanisms();
    Ok(())
}

entry!(main);
