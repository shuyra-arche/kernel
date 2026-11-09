use alloc::sync::Weak;

use kernel_api::model::identity::PendId;
use kernel_api::model::identity::TaskId;
use kernel_api::model::modifiable::Modifiable;
use kernel_api::model::sign::SignSource;
use kernel_api::model::unit::UnitTarget;

use crate::basis::unit::lifecycle::join::JoinTarget;
use crate::basis::unit::task::Task;

#[derive(Clone)]
pub(crate) enum Watch {
    Direct {
        token: kernel_api::model::identity::PendId,
    },
    ExternalMapping {
        token: kernel_api::model::identity::PendId,
        mapping: kernel_api::model::cell::MappingId,
        published: bool,
        generation: u64,
    },
    Nest {
        token: PendId,
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
                published,
                ..
            } => {
                if *published {
                    SignSource::MappingPublished {
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
            Watch::Nest { token, fault } => {
                if *fault {
                    SignSource::NestFault { token: *token }
                } else {
                    SignSource::NestClosed { token: *token }
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
