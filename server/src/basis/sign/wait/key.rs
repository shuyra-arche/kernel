use kernel_api::model::clone_view::CloneCondition;
use kernel_api::model::identity::TaskId;
use kernel_api::model::identity::TeamId;
use kernel_api::model::view::ViewKind;

use crate::basis::nest::Asid;

/// 等待位的键。
///
/// `Mapping` 是内部键：同一段关系上不同页的等待者按整段一起放行，醒后各自重查；
/// 空间清场按 ASID 一并清掉 `Space` 与 `Mapping`。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WakeKey {
    ExternalMapping {
        id: usize,
    },
    Space {
        space: Asid,
        slot: usize,
    },
    Mapping {
        space: Asid,
        mapping: usize,
    },
    Value {
        space: Asid,
        address: usize,
    },
    Clone {
        clone_view: usize,
        dir: CloneCondition,
    },
    Trigger {
        id: usize,
    },
    Task {
        id: TaskId,
    },
    Team {
        id: TeamId,
    },
    Pends {
        task: TaskId,
    },
    Seal {
        kind: u8,
        id: usize,
    },
    Inspect {
        task: TaskId,
        token: usize,
    },
    SignHandle {
        task: TaskId,
        token: usize,
    },
    Sign {
        id: usize,
    },
    Alarm {
        task: TaskId,
    },
}

impl WakeKey {
    pub(crate) fn seal(kind: ViewKind, id: usize) -> Self {
        WakeKey::Seal {
            kind: kind as u8,
            id,
        }
    }

    pub(super) fn fold(self) -> u64 {
        match self {
            WakeKey::ExternalMapping { id } => (id as u64).wrapping_mul(0xB8A7_641D_2C39_EF05),
            WakeKey::Space { space, slot } => {
                (space.get() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ slot as u64
            }
            WakeKey::Value { space, address } => {
                (space.get() as u64).wrapping_mul(0xA24B_AED4_963E_E407) ^ address as u64
            }
            WakeKey::Mapping { space, mapping } => {
                (space.get() as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93) ^ mapping as u64
            }
            WakeKey::Clone { clone_view, dir } => {
                (clone_view as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ dir as u64
            }
            WakeKey::Trigger { id } => (id as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9),
            WakeKey::Team { id } => (id.get() as u64).wrapping_mul(0x369DEA0F31A53F85),
            WakeKey::Task { id } => (id.get() as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93),
            WakeKey::Alarm { task } => (task.get() as u64).wrapping_mul(0xA24B_AED4_963E_E407),
            WakeKey::Pends { task } => (task.get() as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F),
            WakeKey::Seal { kind, id } => {
                (id as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ kind as u64
            }
            WakeKey::Inspect { task, token } => {
                (task.get() as u64).wrapping_mul(0x2545_F491_4F6C_DD1D) ^ token as u64
            }
            WakeKey::SignHandle { task, token } => {
                (task.get() as u64).wrapping_mul(0x517CC1B727220A95) ^ token as u64
            }
            WakeKey::Sign { id } => (id as u64).wrapping_mul(0x1656_67B1_9E37_79F9),
        }
    }
}
