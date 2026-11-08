use std::{ffi::OsStr, path::Path, process::Command};

/// Keep QEMU's real hart numbers: only 1 and 3 enter Kernel, so the boot hart is
/// nonzero and the enabled set is sparse. Lower their advertised MMU ceilings
/// to exercise common-mode selection on hardware supporting all three modes.
pub(super) fn fixture(qemu: &OsStr, path: &Path, io: bool) {
    let machine = if io { "virt,iommu-sys=on" } else { "virt" };
    let mut command = Command::new(qemu);
    command.args([
        "-M",
        &format!("{machine},dumpdtb={}", path.display()),
        "-cpu",
        "rv64",
        "-m",
        "128M",
        "-smp",
        "4",
        "-bios",
        "none",
        "-nographic",
        "-monitor",
        "none",
        "-nic",
        "none",
    ]);
    if io {
        command.args(["-device", "edu,addr=1.0"]);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut bytes = std::fs::read(path).unwrap();
    for id in [0, 2] {
        replace(
            &mut bytes,
            &format!("/cpus/cpu@{id}"),
            "status",
            b"disabled\0",
        );
    }
    replace(&mut bytes, "/cpus/cpu@1", "mmu-type", b"riscv,sv48\0");
    replace(&mut bytes, "/cpus/cpu@3", "mmu-type", b"riscv,sv39\0");
    std::fs::write(path, bytes).unwrap();
}

fn replace(bytes: &mut Vec<u8>, path: &str, name: &str, value: &[u8]) {
    let fdt = fdt::Fdt::new(bytes).unwrap();
    let old = fdt.find_node(path).unwrap().property(name).unwrap().value;
    let at = old.as_ptr() as usize - bytes.as_ptr() as usize;
    let old_size = old.len().next_multiple_of(4);
    let mut data = value.to_vec();
    data.resize(data.len().next_multiple_of(4), 0);
    let delta = data.len() as isize - old_size as isize;
    bytes[at - 8..at - 4].copy_from_slice(&(value.len() as u32).to_be_bytes());
    bytes.splice(at..at + old_size, data);
    for offset in [4, 12, 36] {
        let old = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap());
        bytes[offset..offset + 4].copy_from_slice(&((old as isize + delta) as u32).to_be_bytes());
    }
}

/// Hardware is RVV-capable, but only Harts 0/1 are admitted as V providers.
/// This exercises the scheduler's unavailable-capability boundary without
/// pretending QEMU can configure a different VLEN on each Hart.
pub(super) fn vector_fixture(qemu: &OsStr, path: &Path, cpu: &str) {
    let output = Command::new(qemu)
        .args([
            "-M",
            &format!("virt,dumpdtb={}", path.display()),
            "-cpu",
            cpu,
            "-m",
            "128M",
            "-smp",
            "4",
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
    assert!(output.status.success());
    let mut bytes = std::fs::read(path).unwrap();
    for id in [2, 3] {
        let node_path = format!("/cpus/cpu@{id}");
        let tree = fdt::Fdt::new(&bytes).unwrap();
        let node = tree.find_node(&node_path).unwrap();
        let old = node.property("riscv,isa-extensions").unwrap().value;
        let mut filtered = Vec::new();
        for extension in old.split(|b| *b == 0).filter(|s| !s.is_empty()) {
            if extension == b"v" || extension.starts_with(b"zv") {
                continue;
            }
            filtered.extend_from_slice(extension);
            filtered.push(0);
        }
        replace(&mut bytes, &node_path, "riscv,isa-extensions", &filtered);
        // Keep the ignored legacy property consistent with the modern pair.
        replace(
            &mut bytes,
            &node_path,
            "riscv,isa",
            b"rv64imafdc_zicsr_zifencei_zicntr\0",
        );
    }
    std::fs::write(path, bytes).unwrap();
}

/// Deliberately advertise V on a V-disabled CPU. The bounded boot probe must
/// recover its illegal CSR read and publish no vector capability.
pub(super) fn absent_vector_fixture(qemu: &OsStr, path: &Path, harts: usize) {
    let output = Command::new(qemu)
        .args([
            "-M",
            &format!("virt,dumpdtb={}", path.display()),
            "-cpu",
            "rv64,v=true,vlen=128,elen=64",
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
    assert!(output.status.success());
}
