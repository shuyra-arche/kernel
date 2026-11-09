//! Only a saved trap frame is fault register evidence. Panic stacks are sampled
//! at report entry; neither current CSRs nor reporter GPRs are the fault site.
use super::backtrace::{Backtrace, SPAN, StackReader};
use crate::metal::switcher::context::{Gprs, TrapContext};
use core::fmt::{self, Write};

pub(crate) enum Source<'a> {
    RustPanic(&'a core::panic::PanicInfo<'a>),
    Trap {
        frame: &'a TrapContext,
        cause: usize,
        value: usize,
    },
}

pub(crate) fn write(source: &Source<'_>, out: &mut impl Write, walk: bool) -> fmt::Result {
    let (sp, fp) = match source {
        Source::RustPanic(info) => {
            writeln!(out, "[kernel] source=RustPanic fault-registers=unavailable")?;
            if let Some(location) = info.location() {
                writeln!(out, "location={location}")?;
            }
            writeln!(out, "message={}", info.message())?;
            {
                writeln!(out, "stack-source=DiagnosticCapture")?;
                crate::metal::stop::scene()
            }
        }
        Source::Trap {
            frame,
            cause,
            value,
        } => {
            writeln!(
                out,
                "[kernel] source=Trap pc={:#x} cause={cause:#x} value={value:#x} status={:#x}",
                frame.sepc.as_usize(),
                frame.status()
            )?;
            for (index, value) in frame.gpr.0.iter().enumerate().skip(1) {
                writeln!(out, "x{index}={value:#x}")?;
            }
            (frame.gpr.x(Gprs::SP), frame.gpr.x(Gprs::S0))
        }
    };
    let root_reused = matches!(source, Source::Trap { .. })
        && (crate::metal::image::root_stack_base()..crate::metal::image::root_stack_edge())
            .contains(&sp);
    if root_reused {
        // The trap handler ran on the Hart stack, then reporting reset ROOT's
        // SP. Its old contents can now contain reporter frames, not fault frames.
        writeln!(out, "backtrace unavailable reason=ROOT-reused")
    } else if walk {
        stack(out, sp, fp)
    } else {
        writeln!(out, "backtrace unavailable reason=boot-or-peer-state")
    }
}

fn stack(out: &mut impl Write, sp: usize, fp: usize) -> fmt::Result {
    writeln!(out, "stack sp={sp:#x} fp={fp:#x} completeness=best-effort")?;
    let ceiling = if (crate::metal::image::root_stack_base()
        ..crate::metal::image::root_stack_edge())
        .contains(&sp)
    {
        crate::metal::image::root_stack_edge()
    } else {
        crate::metal::switcher::stack::trap_stack_hart(sp)
            .map(crate::metal::switcher::stack::trap_stack_edge)
            .map(|edge| edge.as_usize())
            .unwrap_or(sp.saturating_add(SPAN))
    };
    // Read-only walk through identity-checked page tables: no allocator or Nest lock.
    let mut reader = StackReader::new(riscv::register::satp::read().ppn());
    let trace = Backtrace::walk(
        |a| reader.word(a),
        sp,
        fp,
        ceiling,
        crate::metal::image::is_code,
    );
    for frame in trace.frames() {
        writeln!(
            out,
            "frame source={:?} pc={:#x} sp={:#x}",
            frame.source, frame.pc, frame.sp
        )?;
    }
    writeln!(
        out,
        "backtrace stop={:?}; symbolize PCs against the exact ELF",
        trace.stop
    )
}
