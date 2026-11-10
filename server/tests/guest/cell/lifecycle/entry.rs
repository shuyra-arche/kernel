use core::fmt::Write;
use kernel_api::{
    address::{PAGE_SIZE, VirtAddr},
    model::unit::{ExitScope, UnitCreate, UnitTarget},
};
use kernel_client::call::unit;

const WORDS: [usize; 3] = [17, 23, 41];
const STACK: usize = 4 * PAGE_SIZE + 1;

pub(crate) fn run() {
    for count in 0..=WORDS.len() {
        let bytes = UnitCreate::Task {
            team: None,
            entry: VirtAddr::new(entry as *const () as usize),
            args: VirtAddr::new(if count == 0 {
                0
            } else {
                WORDS.as_ptr() as usize
            }),
            count: count as u64,
            stack: VirtAddr::new(STACK),
        }
        .to_bytes();
        let target = unit::create(crate::ptr(&bytes)).unwrap();
        let UnitTarget::Task(task) = target else {
            panic!("expected Task");
        };
        unit::resume(target).unwrap();
        assert!(super::wait_task(task));
        assert_eq!(super::result(task), (1, count + 2));
        super::delete(task);
    }
    writeln!(crate::Console, "PASS task-entry stack=1 arguments=1").unwrap();
}

#[unsafe(naked)]
extern "C" fn entry() -> ! {
    core::arch::naked_asm!("mv a2, sp", "tail {check}", check = sym check);
}

extern "C" fn check(args: usize, count: usize, sp: usize) -> ! {
    assert!(count <= WORDS.len());
    assert_eq!(sp, args & !15);
    let top = args + count * size_of::<usize>();
    assert!(top.is_multiple_of(PAGE_SIZE));
    for (index, expected) in WORDS[..count].iter().enumerate() {
        assert_eq!(
            unsafe { (args as *const usize).add(index).read() },
            *expected
        );
    }
    let bottom = (top - STACK.next_multiple_of(PAGE_SIZE)) as *mut usize;
    unsafe { bottom.write_volatile(top) };
    assert_eq!(unsafe { bottom.read_volatile() }, top);
    unit::exit((count + 2) as u64, 0, VirtAddr::new(0), 0, ExitScope::Task)
}
