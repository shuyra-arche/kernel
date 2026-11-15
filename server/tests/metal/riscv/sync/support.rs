pub(crate) mod metal {
    pub(crate) mod generic {
        pub(crate) use crate::provider::generic as sync;
    }
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
    pub(crate) mod sync {
        pub(crate) use crate::provider::sync::*;
    }
}

pub(crate) mod basis {
    pub(crate) use crate::provider::sync;
}
