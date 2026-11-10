pub(crate) mod metal {
    pub(crate) mod interrupt {
        pub(crate) mod window {
            std::thread_local! {
                static NOTICE: std::cell::RefCell<Option<std::sync::mpsc::Sender<()>>> = const { std::cell::RefCell::new(None) };
                static PAUSE: std::cell::RefCell<Option<std::sync::mpsc::Receiver<()>>> = const { std::cell::RefCell::new(None) };
            }
            pub(crate) fn observe(sender: std::sync::mpsc::Sender<()>) {
                NOTICE.with_borrow_mut(|notice| *notice = Some(sender));
            }
            pub(crate) fn pause(
                sender: std::sync::mpsc::Sender<()>,
                resume: std::sync::mpsc::Receiver<()>,
            ) {
                observe(sender);
                PAUSE.with_borrow_mut(|pause| *pause = Some(resume));
            }
            pub(crate) fn poll() {
                NOTICE.with_borrow_mut(|notice| {
                    if let Some(sender) = notice.take() {
                        sender.send(()).unwrap()
                    }
                });
                PAUSE.with_borrow_mut(|pause| {
                    if let Some(resume) = pause.take() {
                        resume
                            .recv_timeout(std::time::Duration::from_secs(5))
                            .unwrap();
                    }
                });
                std::thread::yield_now();
            }
        }
    }
    pub(crate) mod hart {
        use std::cell::Cell;
        std::thread_local! { static ID: Cell<usize> = const { Cell::new(0) }; }
        #[derive(Clone, Copy)]
        pub(crate) struct HartId(usize);
        impl HartId {
            pub(crate) fn get(self) -> usize {
                self.0
            }
        }
        pub(crate) fn hart_id() -> HartId {
            HartId(ID.get())
        }
        pub(crate) fn set(id: usize) {
            ID.set(id);
        }
    }
    pub(crate) mod stop {
        pub(crate) fn hush() {}
    }
    pub(crate) mod sync {
        #[cfg(debug_assertions)]
        macro_rules! depend_enter {
            ($lock:expr) => {
                0usize
            };
        }
        macro_rules! depend_check {
            ($lock:expr, $caller:expr) => {
                #[cfg(debug_assertions)]
                crate::metal::sync::depend::check(
                    $lock as *const _ as *const () as usize,
                    $lock.level,
                    $caller,
                );
            };
        }
        macro_rules! depend_acquire {
            ($lock:expr, $caller:expr) => {
                #[cfg(debug_assertions)]
                crate::metal::sync::depend::acquire(
                    $lock as *const _ as *const () as usize,
                    $lock.level,
                    $caller,
                );
            };
        }
        macro_rules! depend_check_peer {
            ($lock:expr, $lower:expr, $caller:expr) => {
                crate::metal::sync::depend_check!($lock, $caller);
            };
        }
        macro_rules! depend_release {
            ($lock:expr) => {
                #[cfg(debug_assertions)]
                crate::metal::sync::depend::release($lock as *const _ as *const () as usize);
            };
        }
        #[cfg(debug_assertions)]
        pub(crate) use depend_enter;
        pub(crate) use {depend_acquire, depend_check, depend_check_peer, depend_release};
        pub(crate) mod depend {
            use std::cell::RefCell;
            std::thread_local! { static HELD: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) }; }
            #[derive(Debug, Clone, Copy)]
            pub(crate) enum Level {
                Unit,
            }
            #[cfg(debug_assertions)]
            pub(crate) fn check(address: usize, _: Option<Level>, _: usize) {
                HELD.with_borrow(|held| assert!(!held.contains(&address)));
            }
            #[cfg(debug_assertions)]
            pub(crate) fn acquire(address: usize, _: Option<Level>, _: usize) {
                HELD.with_borrow_mut(|held| {
                    assert!(!held.contains(&address));
                    held.push(address);
                });
            }
            #[cfg(debug_assertions)]
            pub(crate) fn release(address: usize) {
                HELD.with_borrow_mut(|held| {
                    let at = held.iter().position(|known| *known == address).unwrap();
                    held.remove(at);
                });
            }
            pub(crate) fn count() -> usize {
                HELD.with_borrow(Vec::len)
            }
        }
        pub(crate) mod trap {
            use std::cell::Cell;
            std::thread_local! { static ENABLED: Cell<bool> = const { Cell::new(true) }; }
            pub(crate) fn enabled() -> bool {
                ENABLED.get()
            }
            pub(crate) struct TrapGuard(bool);
            impl TrapGuard {
                pub(crate) unsafe fn save() -> Self {
                    Self(ENABLED.replace(false))
                }
            }
            impl Drop for TrapGuard {
                fn drop(&mut self) {
                    if self.0 {
                        ENABLED.set(true);
                    }
                }
            }
        }
    }
}
