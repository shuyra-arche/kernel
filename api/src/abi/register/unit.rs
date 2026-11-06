use crate::model::{
    identity::{TaskId, TeamId},
    unit::UnitTarget,
};

impl crate::abi::register::Wire for UnitTarget {
    const WORDS: usize = 2;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        let (tag, id) = match self {
            Self::Task(id) => (0, id.get()),
            Self::Team(id) => (1, id.get()),
        };
        <usize as crate::abi::register::Wire>::pack(&tag, words, at);
        <usize as crate::abi::register::Wire>::pack(&id, words, at);
    }
    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, crate::abi::register::Decode> {
        let tag = <usize as crate::abi::register::Wire>::unpack(words, at)?;
        let id = <usize as crate::abi::register::Wire>::unpack(words, at)?;
        if id == 0 || id > isize::MAX as usize {
            return Err(crate::abi::register::Decode::Invalid);
        }
        match tag {
            0 => Ok(Self::Task(TaskId::new(id))),
            1 => Ok(Self::Team(TeamId::new(id))),
            _ => Err(crate::abi::register::Decode::Invalid),
        }
    }
}
