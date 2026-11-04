#![cfg(test)]

#[path = "../../src/boot/args.rs"]
mod args;

#[test]
fn startup_arguments_preserve_pointer_and_count_order() {
    let values = [0x11usize, 0x22, 0x33];
    args::save_args(values.as_ptr() as usize, values.len());
    assert_eq!(args::args(), values);
    args::save_args(0, 0);
    assert!(args::args().is_empty());
}
