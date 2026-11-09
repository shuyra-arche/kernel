//! Observe relocated U-mode checkpoint PCs without a Kernel testing interface.
use std::{collections::BTreeSet, path::Path, process::Command};
pub(super) fn trace(command: &mut Command, elf: &Path, path: &Path) -> Vec<(String, usize)> {
    let output = Command::new("nm").arg(elf).output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    let address = |name: &str| {
        let line = text
            .lines()
            .find(|l| l.split_whitespace().last() == Some(name))
            .expect(name);
        usize::from_str_radix(line.split_whitespace().next().unwrap(), 16).unwrap()
    };
    let origin = address("realtime_worker_start");
    let points: Vec<_> = [
        "realtime_worker_active",
        "realtime_compute_begin",
        "realtime_preempted",
        "realtime_compute_end",
    ]
    .into_iter()
    .map(|name| (name.to_owned(), 0x40000 + address(name) - origin))
    .collect();
    let ranges = points
        .iter()
        .map(|(_, pc)| format!("0x{pc:x}+4"))
        .collect::<Vec<_>>()
        .join(",");
    command
        .args(["-d", "exec,nochain", "-dfilter", &ranges, "-D"])
        .arg(path);
    points
}
pub(super) fn verify(path: &Path, points: &[(String, usize)], serial: &str) {
    let text = std::fs::read_to_string(path).unwrap();
    let mut order = Vec::new();
    let mut harts = BTreeSet::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("Trace ") else {
            continue;
        };
        let (cpu, _) = rest.split_once(':').unwrap();
        let Some((_, fields)) = rest.split_once('[') else {
            continue;
        };
        let pc = usize::from_str_radix(fields.split('/').nth(1).unwrap(), 16).unwrap();
        if let Some(at) = points.iter().position(|(_, address)| *address == pc) {
            order.push(at);
            harts.insert(cpu.parse::<usize>().unwrap());
        }
    }
    let boot = serial
        .lines()
        .find_map(|l| l.strip_prefix("Boot HART ID"))
        .and_then(|l| l.split(':').nth(1))
        .unwrap()
        .trim()
        .parse::<usize>()
        .unwrap();
    assert_eq!(harts.len(), 1, "admitted checkpoints migrated: {harts:?}");
    assert!(!harts.contains(&boot), "reserved the housekeeping Hart");
    for at in 0..4 {
        assert!(order.contains(&at), "missing checkpoint {}", points[at].0);
    }
    let begin = order.iter().position(|at| *at == 1).unwrap();
    let preempt = order.iter().position(|at| *at == 2).unwrap();
    let end = order.iter().position(|at| *at == 3).unwrap();
    assert!(
        begin < preempt && preempt < end,
        "remote wake did not interrupt the compute loop: {order:?}"
    );
    std::fs::write(
        path.with_extension("summary.txt"),
        format!("boot={boot} admitted={harts:?}\ncheckpoints={points:?}\norder={order:?}\n"),
    )
    .unwrap();
}
