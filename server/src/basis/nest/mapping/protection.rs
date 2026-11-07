//! 映射改权的纯决策：只读取逻辑状态，返回新活动区间和页表更新意图。
//! Nest 在同一持锁区间内准备硬件资源、提交并结算失效；这里不获取锁或操作页表。

use alloc::vec::Vec;

use crate::basis::nest::NestError;
use kernel_api::address::PAGE_SIZE;
use kernel_api::address::VirtAddr;

use super::Access;
use super::ActiveRange;
use super::state::MappingState;

#[derive(Clone, Copy)]
pub(crate) enum Protection {
    SetAccess(Access),
    Narrow(Access),
}

pub(crate) struct PermissionUpdate {
    pub address: VirtAddr,
    pub access: Access,
}

pub(crate) struct ProtectionTransition {
    pub active: Vec<ActiveRange>,
    pub updates: Vec<PermissionUpdate>,
}

impl MappingState {
    /// `base` 与 `window` 来自同一 MappingRange；窗口非空、页对齐且位于初始范围。
    /// 全程只读：拒绝或分配失败都不修改逻辑权限，也不发布任何硬件权限。
    pub(crate) fn protection_transition(
        &self,
        base: VirtAddr,
        (lo, hi): (usize, usize),
        request: Protection,
    ) -> Result<ProtectionTransition, NestError> {
        if let Protection::SetAccess(access) = request
            && !access.is_leaf_set()
        {
            return Err(NestError::NotAligned);
        }
        if !self.covers(lo, hi) {
            return Err(NestError::NoRegion);
        }
        let active = match request {
            Protection::SetAccess(access) => {
                if self
                    .active
                    .iter()
                    .filter(|range| range.overlaps(lo, hi))
                    .any(|range| !range.ceiling.contains(access))
                {
                    return Err(NestError::WidenDenied);
                }
                ActiveRange::split(&self.active, lo, hi, Some(access))?
            }
            Protection::Narrow(ceiling) => ActiveRange::narrow(&self.active, lo, hi, ceiling)?,
        };
        let first = (lo - base.as_usize()) / PAGE_SIZE;
        let pages = (hi - lo + 1) / PAGE_SIZE;
        let borrowed = self.source.is_borrowed();
        let mut updates = Vec::new();
        updates
            .try_reserve(pages)
            .map_err(|_| NestError::OutOfMemory)?;
        for page in first..first + pages {
            let address = base + page * PAGE_SIZE;
            let access =
                ActiveRange::current_at(&active, address.as_usize()).ok_or(NestError::NoRegion)?;
            if borrowed || self.publishes(page) {
                updates.push(PermissionUpdate { address, access });
            }
        }
        Ok(ProtectionTransition { active, updates })
    }
}
