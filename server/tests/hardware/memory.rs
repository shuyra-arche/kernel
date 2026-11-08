use std::{ffi::OsStr, path::Path, process::Command};

/// Preserve QEMU's platform description but split its physical RAM and reserve
/// a page in each bank. The real boot path must survive switching to MMU-backed
/// allocation, including when the bootstrap heap is in the second bank.
pub(super) fn fixture(qemu: &OsStr, path: &Path, harts: usize) {
    let output = Command::new(qemu)
        .args([
            "-M",
            &format!("virt,dumpdtb={}", path.display()),
            "-cpu",
            "rv64",
            "-m",
            "128M",
            "-smp",
            &harts.to_string(),
            "-bios",
            "none",
            "-nographic",
            "-monitor",
            "none",
            "-nic",
            "none",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut bytes = std::fs::read(path).unwrap();
    let fdt = fdt::Fdt::new(&bytes).unwrap();
    let reg = fdt
        .find_node("/memory")
        .unwrap()
        .property("reg")
        .unwrap()
        .value;
    assert_eq!(reg.len(), 16);
    let at = reg.as_ptr() as usize - bytes.as_ptr() as usize;
    // 48 MiB bank, 1 MiB hole, 79 MiB bank. QEMU still backs all physical
    // bytes, but Kernel must never publish the omitted hole to its allocator.
    let tuples = [0x80000000u64, 0x3000000, 0x83100000, 0x4f00000]
        .into_iter()
        .flat_map(u64::to_be_bytes)
        .collect::<Vec<_>>();
    let struct_offset = word(&bytes, 8);
    let strings_offset = word(&bytes, 12);
    let reserve_offset = word(&bytes, 16);
    assert!(reserve_offset < struct_offset && struct_offset < strings_offset);
    bytes[at - 8..at - 4].copy_from_slice(&32u32.to_be_bytes());
    bytes.splice(at..at + 16, tuples);
    let length = word(&bytes, 36) + 16;
    put(&mut bytes, 36, length);
    put(&mut bytes, 12, strings_offset + 16);
    let reservations = [0x82001001u64, 0x100, 0x85001001, 0x100]
        .into_iter()
        .flat_map(u64::to_be_bytes)
        .collect::<Vec<_>>();
    bytes.splice(reserve_offset..reserve_offset, reservations);
    put(&mut bytes, 8, struct_offset + 32);
    put(&mut bytes, 12, strings_offset + 48);
    let size = bytes.len();
    put(&mut bytes, 4, size);
    std::fs::write(path, bytes).unwrap();
}

fn word(bytes: &[u8], at: usize) -> usize {
    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}
fn put(bytes: &mut [u8], at: usize, value: usize) {
    bytes[at..at + 4].copy_from_slice(&(value as u32).to_be_bytes());
}
