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

impl super::Wire for crate::model::unit::ControlId {
    const WORDS: usize = 1;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        self.get().pack(words, at);
    }
    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, super::Decode> {
        usize::unpack(words, at).map(Self::new)
    }
}

impl super::Wire for crate::model::unit::UnitQuery {
    const WORDS: usize = 3;
    fn pack(&self, words: &mut [usize; 6], at: &mut usize) {
        let (tag, a, b) = match *self {
            Self::Caller => (0usize, 0usize, 0usize),
            Self::Target(UnitTarget::Task(id)) => (1, id.get(), 0),
            Self::Target(UnitTarget::Team(id)) => (2, id.get(), 0),
            Self::Children { after } => (3, after.get(), 0),
            Self::Members { team, after } => (4, team.get(), after.get()),
        };
        tag.pack(words, at);
        a.pack(words, at);
        b.pack(words, at);
    }
    fn unpack(words: &[usize; 6], at: &mut usize) -> Result<Self, super::Decode> {
        let tag = usize::unpack(words, at)?;
        let a = usize::unpack(words, at)?;
        let b = usize::unpack(words, at)?;
        if a > isize::MAX as usize || b > isize::MAX as usize {
            return Err(super::Decode::Invalid);
        }
        match (tag, a, b) {
            (0, 0, 0) => Ok(Self::Caller),
            (1, id, 0) if id != 0 => Ok(Self::Target(UnitTarget::Task(TaskId::new(id)))),
            (2, id, 0) if id != 0 => Ok(Self::Target(UnitTarget::Team(TeamId::new(id)))),
            (3, after, 0) => Ok(Self::Children {
                after: TeamId::new(after),
            }),
            (4, team, after) if team != 0 => Ok(Self::Members {
                team: TeamId::new(team),
                after: TaskId::new(after),
            }),
            _ => Err(super::Decode::Invalid),
        }
    }
}
