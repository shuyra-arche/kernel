//! Bounded ELF64 little-endian RV64 load records -> format 2 boot Capsule.
use crate::{Result, require};
fn word(elf: &[u8], at: usize, width: usize) -> Result<usize> {
    let bytes = elf
        .get(at..at.checked_add(width).ok_or("ELF offset overflow")?)
        .ok_or("truncated ELF")?;
    let mut value = [0; 8];
    value[..width].copy_from_slice(bytes);
    Ok(usize::try_from(u64::from_le_bytes(value))?)
}
pub fn initrd(elf: &[u8], version: u8) -> Result<Vec<u8>> {
    encode(elf, version, false)
}
/// The pinned System client fixture materializes all BSS pages in its image.
pub fn system_initrd(elf: &[u8]) -> Result<Vec<u8>> {
    encode(elf, 14, true)
}
fn encode(elf: &[u8], version: u8, dense: bool) -> Result<Vec<u8>> {
    require(matches!(version, 10..=14), "unsupported capsule version")?;
    require(
        elf.get(..6) == Some(b"\x7fELF\x02\x01"),
        "expected ELF64 little endian",
    )?;
    require(word(elf, 18, 2)? == 243, "expected RISC-V ELF")?;
    let entry = word(elf, 24, 8)?;
    let table = word(elf, 32, 8)?;
    let width = word(elf, 54, 2)?;
    let count = word(elf, 56, 2)?;
    require(width >= 56 && count <= 1024, "invalid program headers")?;
    let mut capsule = vec![0; 4096];
    let mut regions = Vec::new();
    for i in 0..count {
        let at = table
            .checked_add(i.checked_mul(width).ok_or("ELF table overflow")?)
            .ok_or("ELF table overflow")?;
        let kind = word(elf, at, 4)?;
        let flags = word(elf, at + 4, 4)?;
        let offset = word(elf, at + 8, 8)?;
        let va = word(elf, at + 16, 8)?;
        let size = word(elf, at + 32, 8)?;
        let memory = word(elf, at + 40, 8)?;
        if kind != 1 || memory == 0 {
            continue;
        }
        require(
            va.is_multiple_of(4096) && size <= memory,
            "invalid ELF load segment",
        )?;
        va.checked_add(memory).ok_or("ELF virtual range overflow")?;
        let pages = memory.div_ceil(4096);
        require(
            !dense || matches!(flags, 5 | 6),
            "invalid System load flags",
        )?;
        let data = if dense { pages } else { size.div_ceil(4096) };
        let payload = if data == 0 { 0 } else { capsule.len() };
        if data != 0 {
            let bytes = elf
                .get(offset..offset.checked_add(size).ok_or("ELF size overflow")?)
                .ok_or("truncated load segment")?;
            let end = payload
                .checked_add(data.checked_mul(4096).ok_or("capsule overflow")?)
                .ok_or("capsule overflow")?;
            require(end <= 512 * 1024 * 1024, "capsule payload exceeds 512 MiB")?;
            capsule.extend_from_slice(bytes);
            capsule.resize(end, 0);
        }
        regions.push([
            va,
            pages,
            data,
            payload,
            1 | if flags & 2 != 0 { 2 } else { 0 } | if flags & 1 != 0 { 4 } else { 0 },
        ]);
    }
    require(
        !regions.is_empty() && 32 + regions.len() * 40 <= 4096,
        "invalid capsule region count",
    )?;
    capsule[..8].copy_from_slice(b"SQBOOT01");
    capsule[8..12].copy_from_slice(&[2, 0, 12, version]);
    capsule[12..16].copy_from_slice(&(regions.len() as u32).to_le_bytes());
    capsule[16..24].copy_from_slice(&(entry as u64).to_le_bytes());
    let length = capsule.len();
    capsule[24..32].copy_from_slice(&(length as u64).to_le_bytes());
    for (i, region) in regions.iter().enumerate() {
        for (j, value) in region.iter().enumerate() {
            let at = 32 + i * 40 + j * 8;
            capsule[at..at + 8].copy_from_slice(&(*value as u64).to_le_bytes());
        }
    }
    let mut initrd = vec![0; 4096];
    initrd[..4].copy_from_slice(&4096u32.to_le_bytes());
    initrd[4..8].copy_from_slice(&u32::try_from(capsule.len())?.to_le_bytes());
    initrd.extend_from_slice(&capsule);
    Ok(initrd)
}
