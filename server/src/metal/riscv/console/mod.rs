use crate::metal::mmu::Address;
use crate::metal::riscv::sbi;
use ::core::fmt;

use sbi::Dbcn;
use sbi::DbcnCall;
use sbi::ecall::SArgs;

mod emergency;
pub use emergency::Emergency;
mod loss;
static LOSS: loss::Loss = loss::Loss::new();
mod line;
mod logger;
mod output;
mod sink;

use line::Line;
pub use logger::init;
pub use sink::Sink;

/// Nonblocking output. Return only bytes acknowledged by firmware, with no
/// implicit newline or UTF-8 replacement. Every submitted chunk stays in one
/// physical page; a zero/partial firmware write returns immediately.
pub fn write(bytes: &[u8]) -> Option<usize> {
    output::transfer(bytes, physical_buffer, |pa, count| {
        DbcnCall::new(Dbcn::ConsoleWrite)
            .args(SArgs {
                a0: count,
                a1: pa,
                ..Default::default()
            })
            .call()
            .ok()
    })
}
fn write_text(text: &str) {
    LOSS.record(text.len(), write(text.as_bytes()));
}

pub fn read(buf: &mut [u8]) -> Option<usize> {
    if buf.is_empty() {
        return None;
    }
    let pa = physical_buffer(buf.as_ptr() as usize, buf.len())?;
    let got = DbcnCall::new(Dbcn::ConsoleRead)
        .args(SArgs {
            a0: buf.len(),
            a1: pa,
            ..Default::default()
        })
        .call()
        .ok()?;
    Some(got.min(buf.len()))
}

fn physical_buffer(va: usize, len: usize) -> Option<usize> {
    if crate::entry::boot::facts::contains_identity(va, len)
        || (crate::entry::boot::facts::dram_edge().is_none()
            && va >= crate::entry::boot::image_region().base
            && va
                .checked_add(len)
                .is_some_and(|end| end <= crate::entry::boot::image_region().range().end))
    {
        Some(va)
    } else {
        translate_kernel(va, len)
    }
}

fn translate_kernel(va: usize, len: usize) -> Option<usize> {
    if crate::metal::mmu::canonical(va).is_user() {
        return None;
    }
    let space = &crate::basis::unit::team::kernel()?.nest;
    let pa0 = space.physical_at(crate::metal::mmu::canonical(va))?;
    let mut va_cur = va;
    let end = va + len;
    while va_cur < end {
        let pa = space.physical_at(crate::metal::mmu::canonical(va_cur))?;
        if pa.as_usize() != pa0.as_usize() + (va_cur - va) {
            return None;
        }
        va_cur = (va_cur & !(kernel_api::address::PAGE_SIZE - 1)) + kernel_api::address::PAGE_SIZE;
    }
    Some(pa0.as_usize())
}

pub fn _write(args: fmt::Arguments) {
    let mut line = Line::new();
    let lost = LOSS.take();
    if lost != 0 {
        let _ = fmt::write(&mut line, format_args!("[kernel] console-lost={lost}\n"));
    }
    let _ = fmt::write(&mut line, args);
    line.emit();
}

#[macro_export]
macro_rules! put {
    ($($arg:tt)*) => { $crate::metal::console::_write(format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! putln {
    () => { $crate::put!("\n") };
    ($($arg:tt)*) => { $crate::metal::console::_write(format_args!("{}\n", format_args!($($arg)*))) };
}
