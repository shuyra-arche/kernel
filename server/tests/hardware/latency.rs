use std::{collections::BTreeMap, path::Path};
pub(super) fn verify(log: &str, output: &Path) {
    assert!(log.contains("bins=log2-saturated-32 proof=observation"));
    let windows = log
        .lines()
        .find_map(|line| line.strip_prefix("latency-windows: opened="))
        .expect("missing reception window evidence");
    let (opened, captured) = windows.split_once(" captured=").unwrap();
    assert!(opened.parse::<u64>().unwrap() > 0);
    assert!(
        captured.trim().parse::<u64>().unwrap() > 0,
        "no interrupt received through a window"
    );
    let mut totals = BTreeMap::<String, u64>::new();
    let mut tsv = String::from("hart\tmetric\tcount\tmax_ticks\tbins\n");
    for line in log.lines().filter(|l| l.starts_with("latency: hart=")) {
        let (head, tail) = line.split_once(" bins=").unwrap();
        let values: Vec<_> = head
            .split_whitespace()
            .skip(1)
            .map(|v| v.split_once('=').unwrap().1)
            .collect();
        let [hart, metric, count, max] = values.as_slice() else {
            panic!("invalid latency record")
        };
        let count = count.parse::<u64>().unwrap();
        let bins: Vec<u64> = tail
            .trim_matches(['[', ']'])
            .split(',')
            .map(|v| v.trim().parse().unwrap())
            .collect();
        assert_eq!(bins.len(), 32);
        assert_eq!(bins.iter().sum::<u64>(), count);
        *totals.entry(metric.to_string()).or_default() += count;
        tsv.push_str(&format!("{hart}\t{metric}\t{count}\t{max}\t{tail}\n"));
    }
    for metric in [
        "irq-off",
        "kernel-active",
        "spin-wait",
        "spin-hold",
        "commit-wait",
        "commit-hold",
        "irq-sign",
        "ready-run",
        "timer-late",
        "probe",
        "outer-wait",
        "outer-hold",
        "ready-queue",
        "dispatch-run",
        "irq-admit",
        "irq-publish",
    ] {
        assert!(
            totals.get(metric).copied().unwrap_or(0) > 0,
            "missing samples for {metric}"
        );
    }
    assert_eq!(totals["outer-wait"], totals["outer-hold"]);
    assert!(
        totals["outer-hold"] < totals["commit-hold"],
        "recursive scopes were not separated"
    );
    assert_eq!(totals["ready-queue"], totals["ready-run"]);
    assert_eq!(totals["dispatch-run"], totals["ready-run"]);
    assert_eq!(totals["irq-admit"], totals["irq-sign"]);
    assert_eq!(totals["irq-publish"], totals["irq-sign"]);
    let outer: Vec<_> = log
        .lines()
        .filter(|line| line.starts_with("latency-outer: "))
        .collect();
    assert!(!outer.is_empty(), "missing outer transaction attribution");
    assert!(
        outer
            .iter()
            .all(|line| line.contains("site=") && line.contains(".rs:"))
    );
    std::fs::write(output.with_extension("outer.txt"), outer.join("\n")).unwrap();
    std::fs::write(output, tsv).unwrap();
}
