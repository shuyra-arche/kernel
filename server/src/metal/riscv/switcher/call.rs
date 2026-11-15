//! RISC-V public-call admission and fixed ECALL instruction width.
pub(crate) fn admitted(cause: usize, supervisor: bool) -> bool {
    cause == 8 && !supervisor
}

pub(crate) fn next_pc(pc: usize, user_end: usize) -> Option<usize> {
    if !pc.is_multiple_of(2) || pc >= user_end {
        return None;
    }
    pc.checked_add(4).filter(|next| *next < user_end)
}
