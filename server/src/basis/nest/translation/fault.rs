use kernel_api::address::{PageOffset, VirtAddr};

use crate::metal::generic::mmu::Attributes;
use crate::metal::generic::mmu::PageTable;
use crate::metal::mmu::Geometry;

use super::super::Access;
use super::NestState;

use crate::metal::generic::trap::FaultKind;

impl FaultKind {
    fn satisfies(self, flags: Attributes) -> bool {
        let need = match self {
            FaultKind::Instruction => Attributes::EXECUTE,
            FaultKind::Load => Attributes::READ,
            FaultKind::Store => Attributes::WRITE,
        };
        flags.contains(need)
    }

    fn allows(self, access: Access) -> bool {
        let need = match self {
            FaultKind::Instruction => Access::EXECUTE,
            FaultKind::Load => Access::READ,
            FaultKind::Store => Access::WRITE,
        };
        access.contains(need)
    }
}

/// 一次用户页错误在纯探针下的结论。
///
/// 它是**读出来**的事实，不是失败码：`Resolved` 已经可用、`Materialize` 有承载
/// 可物化、`Wait` 只能等承载、`Invalid` 不该再续。真正的资源失败（分配耗尽、
/// 同步失败）在完整解析那一步照实报错，不折进这四个之一。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultOutcome {
    /// 身份在、硬件条目已在且满足本次访问：原指令重试即可。
    Resolved,
    /// 身份在、权限也够，但承载不在（墓碑或在途）：按这段关系整段等待。
    /// 载荷是这段关系在占用表里的身份，与空间 ASID 一起定等待位。
    Wait(usize),
    /// 身份在、权限也够、承载可得：走常规物化与发布；有待结算的失效时也归到这里
    /// （PTE 是否还满足这次访问不能单独作为结论，必须先在提交边界外结清再重判）。
    Materialize,
    /// 身份不在，或活动权限不容纳这次访问：不再续，交入口按原样处置。
    Invalid,
}

impl FaultOutcome {
    pub(in crate::basis::nest) fn probe(
        state: &NestState,
        geometry: Geometry,
        address: VirtAddr,
        kind: FaultKind,
    ) -> Self {
        if !state.retirement.invalidation().is_clean() {
            return Self::Materialize;
        }
        let Some(mapping) = state.index.find(address.as_usize()) else {
            return Self::Invalid;
        };
        let inner = mapping.state.lock();
        let Some(range) = inner.range_at(address.as_usize()) else {
            return Self::Invalid;
        };
        if !kind.allows(range.current) {
            return Self::Invalid;
        }
        let page = address.page_align();
        let offset = PageOffset::new(mapping.page_index(page));
        if let Ok((_, flags)) = state
            .root
            .cpu()
            .and_then(|table| table.walk_ref_geometry(page, geometry))
            && kind.satisfies(flags)
        {
            return Self::Resolved;
        }
        if inner.explicit || state.memos.controlled(&mapping, offset.get()) {
            return Self::Wait(mapping.identity());
        }
        match inner.source.can_materialize(offset) {
            Ok(true) => {}
            // 承载已移走，或正被一次移交持有/预留：只能等。
            Ok(false) => return Self::Wait(mapping.identity()),
            Err(_) => return Self::Invalid,
        }
        Self::Materialize
    }
}
