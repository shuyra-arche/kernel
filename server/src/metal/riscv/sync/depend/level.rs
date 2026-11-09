#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(u8)]
pub enum Level {
    Unit = 0,
    Scheduler = 1,
    Address = 2,
    Nest = 3,
    Mapping = 4,
    Flat = 5,
    TeamTasks = 6,
    L3 = 7,
    Registry = 8,
    Asid = 9,
    Frame = 10,
    Tally = 12,
    Spare = 13,
    UnitState = 14,
}
