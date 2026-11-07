#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceKind {
    Supervisor,
    User,
}

impl SpaceKind {
    pub fn is_supervisor(self) -> bool {
        matches!(self, SpaceKind::Supervisor)
    }

    pub(crate) fn spp(self) -> riscv::register::sstatus::SPP {
        if self.is_supervisor() {
            riscv::register::sstatus::SPP::Supervisor
        } else {
            riscv::register::sstatus::SPP::User
        }
    }
}

impl From<kernel_api::model::unit::ProgramKind> for SpaceKind {
    fn from(k: kernel_api::model::unit::ProgramKind) -> Self {
        match k {
            kernel_api::model::unit::ProgramKind::User => SpaceKind::User,
            kernel_api::model::unit::ProgramKind::Supervisor => SpaceKind::Supervisor,
        }
    }
}
