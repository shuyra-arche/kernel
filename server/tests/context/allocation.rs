use super::extension::{Capability, ExtendedContext, Shape};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
thread_local! {
    static BUDGET: Cell<Option<usize>> = const { Cell::new(None) };
    static LIVE: Cell<isize> = const { Cell::new(0) };
}
struct Gate;
#[global_allocator]
static ALLOCATOR: Gate = Gate;
unsafe impl GlobalAlloc for Gate {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let permitted = BUDGET
            .try_with(|b| match b.get() {
                Some(0) => false,
                Some(n) => {
                    b.set(Some(n - 1));
                    true
                }
                None => true,
            })
            .unwrap_or(true);
        if !permitted {
            return std::ptr::null_mut();
        }
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            account(layout.size() as isize);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        account(-(layout.size() as isize));
        unsafe { System.dealloc(ptr, layout) };
    }
}
fn account(bytes: isize) {
    let _ = BUDGET.try_with(|b| {
        if b.get().is_some() {
            let _ = LIVE.try_with(|v| v.set(v.get() + bytes));
        }
    });
}
fn scoped(budget: usize, f: impl FnOnce()) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            BUDGET.with(|b| b.set(None));
        }
    }
    LIVE.with(|v| v.set(0));
    BUDGET.with(|b| b.set(Some(budget)));
    let reset = Reset;
    f();
    let live = LIVE.with(Cell::get);
    drop(reset);
    assert_eq!(live, 0, "extended allocation leaked");
}

#[test]
fn failed_first_use_does_not_publish_status_or_partial_images() {
    let caps = Capability {
        float: true,
        vector: Shape::checked(32, 15),
    };
    // vfadd.vv requests both images. Fail before FP or after FP/before vector.
    for budget in [0, 1] {
        scoped(budget, || {
            let mut state = ExtendedContext::new();
            let mut status = 0x20;
            assert_eq!(state.enable(0x02001057, &mut status, caps), Err(()));
            assert_eq!(status, 0x20);
            assert!(state.compatible(Capability {
                float: false,
                vector: None
            }));
            assert_eq!(LIVE.with(Cell::get), 0);
            BUDGET.with(|b| b.set(Some(usize::MAX)));
            assert_eq!(state.enable(0x02001057, &mut status, caps), Ok(true));
            assert_eq!(status & 0x6600, 0x4400);
            assert!(LIVE.with(Cell::get) >= 32 * 32 + 32 * 8);
        });
    }
}

#[test]
fn detaching_the_execution_image_releases_it_before_result_retirement() {
    scoped(usize::MAX, || {
        let caps = Capability {
            float: true,
            vector: Shape::checked(16, 15),
        };
        let mut retained = ExtendedContext::new();
        let mut status = 0;
        retained.enable(0x02001057, &mut status, caps).unwrap();
        assert!(LIVE.with(Cell::get) > 0);
        drop(core::mem::take(&mut retained));
        assert_eq!(LIVE.with(Cell::get), 0);
        assert!(retained.compatible(Capability {
            float: false,
            vector: None
        }));
    });
}

#[test]
fn vector_allocation_failure_preserves_an_existing_float_image() {
    scoped(usize::MAX, || {
        let caps = Capability {
            float: true,
            vector: Shape::checked(32, 15),
        };
        let mut state = ExtendedContext::new();
        let mut status = 0;
        assert_eq!(state.enable(0xf2000053, &mut status, caps), Ok(true));
        let before = status;
        let live = LIVE.with(Cell::get);
        BUDGET.with(|b| b.set(Some(0)));
        assert_eq!(state.enable(0xc2202573, &mut status, caps), Err(()));
        assert_eq!(status, before);
        assert_eq!(LIVE.with(Cell::get), live);
        assert!(state.compatible(Capability {
            float: true,
            vector: None
        }));
        assert!(!state.compatible(Capability {
            float: false,
            vector: None
        }));
    });
}
