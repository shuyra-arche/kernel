use super::*;

pub struct Join<T> {
    pub(super) slot: *mut Completion<T>,
    /// 子任务句柄（spawn 时 envcall 返的）——**就是喂给 `accord` 的那个类型**，
    /// 故以句柄存，不化回裸数。
    pub(super) id: TaskId,
}

impl<T> Join<T> {
    /// 子任务句柄——直接可喂 `pie.accord(join.id(), subset, Mark::NONE)`（记号照源枚）。
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// Wait for the Rust value; native Reaped without DONE reports abnormal exit.
    pub fn join(self) -> T {
        sweep();
        loop {
            if unsafe { (*self.slot).state.load(Ordering::Acquire) } & DONE != 0 {
                let slot = self.slot;
                core::mem::forget(self);
                let result = unsafe { (*slot).result.take() }.expect("joined task lost result");
                unsafe {
                    drop(Box::from_raw(slot));
                }
                return result;
            }
            // Native completion also covers faults before the closure writes T.
            // Recheck DONE after observing Reaped: the child could have finished
            // between the first load and the kernel observation.
            if settle_owned(self.id)
                && unsafe { (*self.slot).state.load(Ordering::Acquire) } & DONE == 0
            {
                let slot = self.slot;
                core::mem::forget(self);
                unsafe {
                    drop(Box::from_raw(slot));
                }
                panic!("joined task exited without a Rust result");
            }
            let _ = wait(self.slot as usize, Wait::AtMost(1_000));
        }
    }
}

impl<T> Drop for Join<T> {
    fn drop(&mut self) {
        sweep();
        let previous = unsafe { (*self.slot).state.fetch_or(LEFT, Ordering::AcqRel) };
        if previous & DONE != 0 {
            unsafe {
                drop(Box::from_raw(self.slot));
            }
        } else {
            DETACHED.with(|list| {
                list.push(Detached {
                    task: self.id,
                    slot: self.slot as usize,
                    release: release::<T>,
                })
            });
        }
    }
}
