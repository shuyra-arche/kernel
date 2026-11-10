use super::*;
use kernel_client::call::flat;
pub(super) fn run(nest: PendId) {
    let root = flat::create(crate::license::get(), 4096, 4096, 0).unwrap();
    let slot = flat::slice(root, 0, 1).unwrap();
    let position = create(nest, slot, ADDRESS, true);
    publish(position);
    let end = room::clock() + 2_000_000_000;
    loop {
        match flat::block(root) {
            Ok(()) => break,
            Err(error) => {
                assert_eq!(error.source, kernel_api::abi::call::flat::FlatFail::Busy);
                assert!(room::clock() < end, "External Block did not settle");
                room::yield_();
            }
        }
    }
    assert_eq!(super::super::info(position).state, CellState::Ready);
    assert!(!super::super::info(position).external);
    assert!(cell::publish(super::super::ptr(&current(position).to_bytes())).is_err());
    flat::allow(root).unwrap();
    publish(position);
    retract(position);
    super::super::delete(position.object);
    super::super::delete(CellObject::Slot(slot));
    flat::delete(root).unwrap();
    returned(root);
    use core::fmt::Write;
    let _ = writeln!(
        super::super::Console,
        "PASS external-block retained=1 fenced=1 republish=1"
    );
}
