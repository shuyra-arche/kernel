use alloc::sync::Weak;

use kernel_api::{
    model::identity::PendId, model::identity::TaskId, model::modifiable::Modifiable,
    model::sign::SignSource, model::unit::UnitTarget,
};

use crate::core::work::unit::join::JoinTarget;
use crate::core::work::unit::task::Task;

#[derive(Clone)]
pub(crate) enum Watch {
    Direct {
        token: kernel_api::model::sign::SignId,
    },
    ExternalMapping {
        token: kernel_api::model::identity::PendId,
        mapping: kernel_api::model::memory::MappingId,
        fault: bool,
    },
    Resource {
        source: SignSource,
    },
    Join {
        target: UnitTarget,
        selected: JoinTarget,
    },
    Inspect {
        task: TaskId,
        token: PendId,
        holder: Weak<Task>,
        parent: Option<(Weak<Task>, PendId)>,
        held: bool,
        access: Modifiable,
    },
}

impl Watch {
    pub(crate) fn source(&self) -> SignSource {
        match self {
            Watch::Direct { token } => SignSource::Sign { token: *token },
            Watch::ExternalMapping {
                token,
                mapping,
                fault,
            } => {
                if *fault {
                    SignSource::MappingFault {
                        token: *token,
                        mapping: *mapping,
                    }
                } else {
                    SignSource::MappingRetired {
                        token: *token,
                        mapping: *mapping,
                    }
                }
            }
            Watch::Resource { source } => *source,
            Watch::Join { target, .. } => SignSource::Join { target: *target },
            Watch::Inspect { task, token, .. } => SignSource::Inspect {
                task: *task,
                token: *token,
            },
        }
    }
}
