//! The QEMU/OpenSBI test platform explicitly vouches for its CSR permissions.
use std::{ffi::OsStr, path::Path, process::Command};

pub(super) fn fixture(
    qemu: &OsStr,
    path: &Path,
    kernel: &Path,
    cpu: &str,
    harts: usize,
    permissions: &[&str],
) {
    let output = Command::new(qemu)
        .args([
            "-M",
            &format!("virt,dumpdtb={}", path.display()),
            "-cpu",
            cpu,
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
            "-kernel",
        ])
        .arg(kernel)
        .args(["-append", "clock-fixture"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let mut bytes = std::fs::read(path).unwrap();
    for name in permissions {
        chosen_boolean(&mut bytes, name);
    }
    std::fs::write(path, bytes).unwrap();
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn chosen_boolean(bytes: &mut Vec<u8>, name: &str) {
    let tree = fdt::Fdt::new(bytes).unwrap();
    let chosen = tree.find_node("/chosen").unwrap();
    assert!(chosen.property(name).is_none());
    let anchor = chosen.property("bootargs").unwrap().value.as_ptr() as usize;
    let position = anchor - bytes.as_ptr() as usize - 12;
    assert_eq!(word(bytes, position), 3); // FDT_PROP

    let strings = word(bytes, 12);
    let strings_size = word(bytes, 32);
    let name_at = (strings + strings_size) as usize;
    bytes.splice(name_at..name_at, name.bytes().chain([0]));
    let property = [3u32, 0, strings_size]
        .into_iter()
        .flat_map(u32::to_be_bytes);
    bytes.splice(position..position, property);

    for (offset, extra) in [
        (4, 12 + name.len() + 1), // totalsize
        (12, 12),                 // off_dt_strings
        (32, name.len() + 1),     // size_dt_strings
        (36, 12),                 // size_dt_struct
    ] {
        let updated = word(bytes, offset) + extra as u32;
        bytes[offset..offset + 4].copy_from_slice(&updated.to_be_bytes());
    }
}

pub(super) fn verify(log: &str, direct: bool) {
    let report = log
        .lines()
        .find(|line| line.starts_with("timer-program:"))
        .expect("missing comparator measurements");
    let value = |key: &str| -> u64 {
        report
            .split_whitespace()
            .find_map(|field| field.strip_prefix(key))
            .unwrap()
            .parse()
            .unwrap()
    };
    assert_eq!(value("sbi=") > 0, !direct, "{report}");
    assert_eq!(value("sstc=") > 0, direct, "{report}");
    assert!(value("skipped=") > 0, "{report}");

    let timer = log.lines().find(|line| line.starts_with("timer:")).unwrap();
    let remote: u64 = timer
        .split_once("remote_rearms=")
        .unwrap()
        .1
        .parse()
        .unwrap();
    assert!(remote > 0, "{timer}");
}
