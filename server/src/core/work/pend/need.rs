use kernel_api::model::modifiable::Modifiable;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Need {
    Read,
    Write,
}
impl Need {
    pub(crate) fn allowed_by(self, access: Modifiable) -> bool {
        match self {
            Self::Write => access == Modifiable::Yes,
            Self::Read => true,
        }
    }
}
