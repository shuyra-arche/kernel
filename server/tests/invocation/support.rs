use kernel_api::{abi::memory::journal::Journal, address::VirtAddr, model::identity::TaskId};
use std::{
    cell::Cell,
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
thread_local! { static DEPTH: Cell<usize> = const { Cell::new(0) }; }
static COMMIT: Mutex<()> = Mutex::new(());
pub(crate) struct Commit {
    _guard: Option<MutexGuard<'static, ()>>,
}
impl Drop for Commit {
    fn drop(&mut self) {
        DEPTH.with(|n| n.set(n.get() - 1));
    }
}
pub(crate) fn commit_depth() -> usize {
    DEPTH.with(Cell::get)
}
pub(crate) fn commit() -> Commit {
    let guard = (commit_depth() == 0).then(|| COMMIT.lock().unwrap());
    DEPTH.with(|n| n.set(n.get() + 1));
    Commit { _guard: guard }
}
pub(crate) struct Lock<T>(pub Mutex<T>);
impl<T> Lock<T> {
    pub(crate) fn lock(&self) -> MutexGuard<'_, T> {
        self.0.lock().unwrap()
    }
}
pub(crate) struct Task {
    pub ident: Arc<TaskIdent>,
    pub pend: Lock<()>,
    pub memory_results: Lock<Option<Box<Journal>>>,
    pub effects: AtomicUsize,
    pub mode: AtomicUsize,
    pub completion: Lock<Option<crate::memory_result::Completion>>,
}
impl Task {
    pub fn new() -> Arc<Self> {
        Arc::new_cyclic(|weak| Self {
            ident: Arc::new(TaskIdent {
                id: TaskId::new(1),
                task: weak.clone(),
                team: Team {
                    nest: Buffer {
                        bytes: Mutex::new([0; 512]),
                        fail: AtomicBool::new(false),
                    },
                },
            }),
            pend: Lock(Mutex::new(())),
            memory_results: Lock(Mutex::new(None)),
            effects: AtomicUsize::new(0),
            mode: AtomicUsize::new(0),
            completion: Lock(Mutex::new(None)),
        })
    }
    pub fn realtime(&self) -> bool {
        false
    }
}
pub(crate) struct TaskIdent {
    pub id: TaskId,
    pub task: Weak<Task>,
    pub team: Team,
}
pub(crate) struct Team {
    pub nest: Buffer,
}
pub(crate) struct Buffer {
    pub bytes: Mutex<[u8; 512]>,
    pub fail: AtomicBool,
}
impl Buffer {
    pub fn copy_in(&self, output: &mut [u8], at: VirtAddr) -> bool {
        let Some(start) = at.get().checked_sub(0x1000) else {
            return false;
        };
        let bytes = self.bytes.lock().unwrap();
        let Some(data) = bytes.get(start..start + output.len()) else {
            return false;
        };
        output.copy_from_slice(data);
        true
    }
    pub fn copy_out(&self, input: &[u8], at: VirtAddr) -> bool {
        if self.fail.load(Ordering::SeqCst) {
            return false;
        }
        let Some(start) = at.get().checked_sub(0x1000) else {
            return false;
        };
        let mut bytes = self.bytes.lock().unwrap();
        let Some(data) = bytes.get_mut(start..start + input.len()) else {
            return false;
        };
        data.copy_from_slice(input);
        true
    }
}
pub(crate) enum Gprs {
    A0,
    A1,
    A2,
}
#[derive(Default)]
pub(crate) struct Registers(pub [usize; 3]);
impl Registers {
    pub fn set_x(&mut self, reg: Gprs, value: usize) {
        self.0[reg as usize] = value;
    }
}
#[derive(Default)]
pub(crate) struct TrapContext {
    pub gpr: Registers,
}
