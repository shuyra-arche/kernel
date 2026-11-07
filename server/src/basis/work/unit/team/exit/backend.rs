pub(super) fn stop(reason: usize) -> ! {
    crate::entry::report::halt::protected_stop(reason)
}
