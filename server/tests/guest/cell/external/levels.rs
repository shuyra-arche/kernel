use super::*;
use core::fmt::Write;
use kernel_api::model::nest::NestKind;
use kernel_client::call::pend;
pub(super) fn run(bar: VirtAddr, port: PendId) {
    let root = nest::create(crate::license::get(), NestKind::External).unwrap();
    nest::attach(root, port).unwrap();
    let slot = crate::levels::allocate(1, 1);
    let (cpu, address) = crate::levels::attach_large(slot);
    crate::write(address, 0, 0x87654321);
    crate::write(address, (1 << 21) - 4096, 0);
    crate::retract(cpu, 0);
    let position = create(root, slot, ADDRESS, true);
    assert_eq!(crate::info(position).level.get(), 1);
    publish(position);
    assert!(cell::split(slot).is_err());
    transfer(bar, ADDRESS, false);
    transfer(bar, ADDRESS + (1 << 21) - 4096, true);
    assert_eq!(context(root).fault_sequence, 0);
    start(bar, ADDRESS + (1 << 21) - 4096, false);
    retract(position);
    assert_eq!(read(bar, 0x98) & 1, 0);
    assert!(!crate::info(position).external);
    crate::delete(position.object);
    nest::detach(root).unwrap();
    closed(root);
    nest::delete(root).unwrap();
    pend::return_(root).unwrap();
    crate::publish(cpu, 0);
    assert_eq!(crate::read(address, (1 << 21) - 4096), 0x87654321);
    crate::retract(cpu, 0);
    crate::delete(crate::raw_memo(cpu, 0).object);
    let children = cell::split(slot).unwrap();
    crate::delete(CellObject::Slot(children));
    let _ = writeln!(
        crate::Console,
        "PASS cell-io-levels last-leaf=1 retirement=1 split=1"
    );
}
