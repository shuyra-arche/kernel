use sbi;
use sbi::ecall::SArgs;
use sbi::fid;

use crate::basis::nest::NestError;
use crate::machine::hart;

use super::asid::Asid;
use super::asid::VACANT;
use super::error::Deaf;

pub fn deallocate(asid: Asid) -> Result<(), Deaf> {
    shootdown(asid)?;
    Asid::release(asid);
    Ok(())
}

pub fn occupy(asid: Asid) {
    hart::lease_store(asid.get());
    // 发布租约数据；与 satp/本地失效的相对次序由调用点保证。
    fence_data();
}

pub fn vacate() {
    hart::lease_store(VACANT);
}

/// 排序此前页表/租约写与后续显式数据访问；本地翻译失效另由 flush 承担。
#[inline(always)]
fn fence_data() {
    unsafe { core::arch::asm!("fence rw, rw", options(nostack)) };
}

pub(crate) fn lease(hart: crate::machine::hart::HartId) -> Option<Asid> {
    match hart::lease_load(hart) {
        VACANT => None,
        raw => Some(Asid::from_raw(raw)),
    }
}

pub fn shootdown(asid: Asid) -> Result<(), Deaf> {
    crate::machine::allocator::assert_allocation_allowed();
    // 调用方已经写下的 PTE 排在本地 flush 与下面那次租约扫描之前：漏掉这一序，
    // 远端可能按旧翻译继续用这条 ASID。
    fence_data();
    unsafe { flush(asid) };

    let me = hart::hart_id();
    let operation = if asid.is_kernel() {
        fid::Rfence::RemoteSfenceVma
    } else {
        fid::Rfence::RemoteSfenceVmaAsid
    };
    for slot in (0..hart::hart_count()).map(hart::HartId::new) {
        if slot == me {
            continue;
        }
        if (asid.is_kernel() && hart::online(slot)) || lease(slot) == Some(asid) {
            let (mask, base) = hart::firmware(slot).target();
            sbi::RfenceCall::new(operation)
                .args(SArgs {
                    a0: mask,
                    a1: base,
                    a2: 0,
                    a3: 0,
                    a4: asid.get(),
                    ..Default::default()
                })
                .call()
                .map_err(|_| Deaf { asid })?;
        }
    }
    Ok(())
}

/// 本 hart 直接失效该 ASID 的 TLB（shootdown 的本地一半）。
#[inline(always)]
pub unsafe fn flush(asid: Asid) {
    unsafe {
        if asid.is_kernel() {
            core::arch::asm!("sfence.vma zero, zero");
        } else {
            core::arch::asm!("sfence.vma zero, {}", in(reg) asid.get());
        }
    }
}

/// 让已写下的指令字节对所有在线地址空间的 hart 可见。
pub fn sync_instructions() -> Result<(), NestError> {
    unsafe { core::arch::asm!("fence rw, rw", options(nostack)) };
    hart::request_instruction_sync();
    unsafe { core::arch::asm!("fence rw, rw", "fence.i", options(nostack)) };
    let me = hart::hart_id();
    for slot in (0..hart::hart_count()).map(hart::HartId::new) {
        if slot != me && lease(slot).is_some() {
            let (mask, base) = hart::firmware(slot).target();
            sbi::RfenceCall::new(sbi::fid::Rfence::RemoteFenceI)
                .args(SArgs {
                    a0: mask,
                    a1: base,
                    ..Default::default()
                })
                .call()
                .map_err(|_| NestError::NoRegion)?;
        }
    }
    Ok(())
}
