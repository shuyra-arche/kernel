//! Observe guest-only checkpoint PCs through QEMU, without a test syscall or
//! production hook. Each checkpoint has exactly one successful Task caller.
use std::{collections::BTreeSet, path::Path, process::Command};
pub(super) fn trace(
    command: &mut Command,
    elf: &Path,
    output: &Path,
    vector: bool,
    mixed: bool,
) -> Vec<(String, usize)> {
    let mut names = vec!["context_float_one", "context_float_two"];
    if vector {
        names.push("context_vector_one");
        if !mixed {
            names.push("context_vector_two");
        }
    }
    trace_checkpoints(command, elf, output, &names)
}

pub(super) fn trace_checkpoints(
    command: &mut Command,
    elf: &Path,
    output: &Path,
    names: &[&str],
) -> Vec<(String, usize)> {
    let symbols = Command::new("nm").arg(elf).output().expect("nm");
    assert!(symbols.status.success());
    let symbols = String::from_utf8(symbols.stdout).unwrap();
    let found: Vec<_> = names
        .iter()
        .copied()
        .map(|name| {
            let line = symbols
                .lines()
                .find(|line| line.split_whitespace().last() == Some(name))
                .expect(name);
            (
                name.to_owned(),
                usize::from_str_radix(line.split_whitespace().next().unwrap(), 16).unwrap(),
            )
        })
        .collect();
    let ranges = found
        .iter()
        .map(|(_, pc)| format!("0x{pc:x}+4"))
        .collect::<Vec<_>>()
        .join(",");
    command
        .args(["-d", "exec,nochain", "-dfilter", &ranges, "-D"])
        .arg(output);
    found
}
pub(super) fn verify(path: &Path, symbols: &[(String, usize)], mixed: bool) {
    let trace = std::fs::read_to_string(path).unwrap();
    let mut evidence = String::new();
    for (name, address) in symbols {
        let mut harts = BTreeSet::new();
        for line in trace.lines() {
            let Some(rest) = line.strip_prefix("Trace ") else {
                continue;
            };
            let Some((cpu, _)) = rest.split_once(':') else {
                continue;
            };
            let Some((_, fields)) = rest.split_once('[') else {
                continue;
            };
            let Some(pc) = fields.split('/').nth(1) else {
                continue;
            };
            if usize::from_str_radix(pc, 16).ok() == Some(*address) {
                harts.insert(cpu.parse::<usize>().unwrap());
            }
        }
        assert!(
            harts.len() >= 2,
            "{name} did not migrate: {harts:?}; {}",
            path.display()
        );
        if mixed && name.starts_with("context_vector") {
            assert_eq!(
                harts,
                BTreeSet::from([0, 1]),
                "vector ran on undeclared Hart"
            );
        }
        evidence.push_str(&format!("{name} pc={address:#x} Harts={harts:?}\n"));
    }
    std::fs::write(path.with_extension("summary.txt"), evidence).unwrap();
}
