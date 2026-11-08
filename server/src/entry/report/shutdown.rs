use crate::basis::room::deadline;
use crate::basis::room::halt;
use crate::basis::room::interrupt;
use crate::basis::room::scheduler;
use crate::basis::unit::lifecycle::tally;
use crate::metal::clock;
use crate::putln;

pub(crate) fn waiting() {
    let (arrived, total) = halt::barrier();
    let (pushed, reaped) = tally::counts();
    putln!(
        "[stop] halt 屏障等待：已达 {arrived}/{total} 核；任务 PUSHED={pushed}                      REAPED={reaped}（差 {}）—— 屏障等的是**核**，不是任务",
        pushed.saturating_sub(reaped)
    );
}

pub(crate) fn report() {
    putln!("task: all tasks exited, system halted");
    let (late_n, late_max, late_sum) = deadline::late_stats();
    let max_ms = clock::ticks_to_duration(late_max).as_millis();
    let avg_ms = if late_n == 0 {
        0
    } else {
        clock::ticks_to_duration(late_sum / late_n).as_millis()
    };
    let (tocks, mutes, remote_rearms) = deadline::tock_stats();
    putln!(
        "timer: late_n={late_n} late_max_ms={max_ms} late_avg_ms={avg_ms} late_max_tick={late_max} traps={} tocks={tocks} mutes={mutes} remote_rearms={remote_rearms}",
        deadline::ticks()
    );
    #[cfg(debug_assertions)]
    {
        let ([(sbi, sbi_ticks), (sstc, sstc_ticks)], skipped) = clock::timer::measures();
        putln!(
            "timer-program: sbi={sbi} sbi_ticks={sbi_ticks} sstc={sstc} sstc_ticks={sstc_ticks} skipped={skipped}"
        );
    }
    let (held, starved, blocked, nudged) = crate::basis::unit::lifecycle::branch_stats();
    putln!("doom: held={held} starved={starved} blocked={blocked} nudged={nudged}");
    let (kicks, fallback) = scheduler::kick_stats();
    putln!("sched: kicks={kicks} fallback={fallback}");
    let (ring, busy, idle_ring, idle_busy) = interrupt::stats();
    putln!(
        "supervisor_external: ring={ring} busy={busy} idle_ring={idle_ring} idle_busy={idle_busy}"
    );
    crate::basis::view::clone::hold_line();
    use crate::metal::allocator::statistics;
    putln!(
        "memory: frames={} pools={}",
        statistics::frame_occupied(),
        statistics::block_occupied()
    );
    #[cfg(debug_assertions)]
    putln!(
        "memory: spare_used={} spare_available={} kinds={}",
        statistics::spare_occupied(),
        statistics::spare_available(),
        statistics::kinds()
    );
    crate::basis::unit::lifecycle::diagnostics::each(|entry| {
        putln!(
            "unclaimed exit: task={} reason={} note={}",
            entry.task.get(),
            entry.reason,
            entry.note()
        );
    });
    #[cfg(feature = "semihosting")]
    super::export::complete();
}
