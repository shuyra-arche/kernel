pub(crate) const ROOT_STACK_SIZE: usize = 0x1_0000;
pub(crate) const ROOT_STACK_CANARY: usize = 0x600D_CAFE_51A7_0D1E;

pub(crate) fn kernel_edge() -> usize {
    (&raw const _kernel_edge).addr()
}
unsafe extern "C" {
    static _kernel_edge: u8;
}

#[unsafe(no_mangle)]
static _stack: usize = ROOT_STACK_SIZE;

#[unsafe(no_mangle)]
static _canary: usize = ROOT_STACK_CANARY;

pub(crate) fn root_stack_base() -> usize {
    kernel_edge()
}

pub(crate) fn root_stack_edge() -> usize {
    kernel_edge() + ROOT_STACK_SIZE
}

/// ROOT and other writable image data are not executable addresses.
pub(crate) fn is_code(pc: usize) -> bool {
    unsafe extern "C" {
        static _kernel_base: u8;
        static _text_end: u8;
    }
    ((&raw const _kernel_base).addr()..(&raw const _text_end).addr()).contains(&pc)
}
