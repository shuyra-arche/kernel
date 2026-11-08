use alloc::sync::Arc;
use riscv::register::scause;
use riscv::register::sepc;
use riscv::register::stval;

use crate::basis::nest::{FaultKind, FaultOutcome, Nest};
use crate::basis::room::Handoff;
use crate::basis::room::conductor::identity::Identity;
use crate::basis::sign::{self, WakeKey};
use crate::machine::switcher::context::TrapContext;
use crate::putln;

use super::{kill_fault, resume};
use kernel_api::address::VirtAddr;

#[derive(Debug)]
pub(super) struct PageFault {
    pub addr: VirtAddr,
    pub pc: usize,
    pub kind: FaultKind,
}

impl PageFault {
    pub(super) unsafe fn capture() -> Self {
        let code = scause::read().code();
        let kind = match code {
            12 => FaultKind::Instruction,
            13 => FaultKind::Load,
            15 => FaultKind::Store,
            _ => panic!("capture() called on non-page-fault scause={}", code),
        };

        Self {
            addr: crate::machine::mmu::riscv::canonical(stval::read()),
            pc: sepc::read(),
            kind,
        }
    }
}

/// 一次页错误的等待上下文：纯探针重查所需的地址空间、地址、访问种类与
/// **本次登记的那段关系身份**。
///
/// 它是条件等待谓词的整个捕获，谓词只在 work 的提交边界内跑一次：这里不推进
/// 失效、不物化，只有 `Nest::probe_fault` 这一个只读口。
struct FaultWait {
    nest: Arc<Nest>,
    address: VirtAddr,
    kind: FaultKind,
    expected: usize,
}

impl FaultWait {
    fn new(nest: Arc<Nest>, address: VirtAddr, kind: FaultKind, expected: usize) -> Self {
        Self {
            nest,
            address,
            kind,
            expected,
        }
    }

    /// 只有“这一页仍然只能等，而且等的正是本次这段关系”才算还没就绪。
    ///
    /// 身份变了就必须立刻回去重探：旧身份已经被清过场、也 signal 过了，
    /// 再睡在旧键上就永远等不到人；新身份的可物化、已经可用或已消失都是重探
    /// 就能决定的。
    fn ready(&self) -> bool {
        !matches!(
            self.nest.probe_fault(self.address, self.kind),
            FaultOutcome::Wait(mapping) if mapping == self.expected
        )
    }
}

/// 按这段关系整段等一次承载；登记与切换都不带着调用者的强引用。
pub(super) fn wait(
    ident: Option<Identity>,
    fault: PageFault,
    nest: Arc<Nest>,
    mapping: usize,
    frame: &mut TrapContext,
) -> *mut TrapContext {
    // 登记失败要终止任务时，身份已经交出去了：先把这一 id 标量留下来。
    let tid = ident.as_ref().map_or(0, Identity::task_id);
    let life = nest.life();
    let key = WakeKey::Mapping {
        space: nest.asid(),
        mapping,
    };
    let waiting = FaultWait::new(nest, fault.addr, fault.kind, mapping);
    // 先放掉任务身份；地址探针在提交边界内执行后释放。
    drop(ident);
    match sign::wait_until::<kernel_api::abi::call::room::RoomFail, _>(
        key,
        life,
        waiting,
        FaultWait::ready,
    ) {
        Ok(Handoff::Resume(())) => resume(frame as *mut TrapContext),
        Ok(Handoff::Switch(pa)) => pa as *mut TrapContext,
        Err(resource) => {
            log::error!("page fault wait could not be registered: {resource:?}");
            killed(None, tid, fault)
        }
    }
}

/// 按原样处置：记录真杀事件并终止这个任务。
pub(super) fn killed(ident: Option<Identity>, tid: usize, fault: PageFault) -> *mut TrapContext {
    let cause_bits = scause::read().bits();
    let stval_bits = stval::read();
    log::error!(
        "user page fault killed: {:?} at {:?} pc={:#x}",
        fault.kind,
        fault.addr,
        fault.pc
    );
    putln!("user fault killed: tid={tid} cause={cause_bits} stval={stval_bits:#x}");
    kill_fault(ident)
}
