use kernel_verification::Result;
use regex::Regex;
use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub enum Kind {
    Nest,
    Flat,
}

pub fn check(log: &str, kind: &Kind, harts: u8) -> Result<Value> {
    let mut markers = vec!["task: all tasks exited".to_owned()];
    let mut evidence = json!({});
    let valid = match kind {
        Kind::Nest => {
            markers.extend([
                "PASS nest-completion abi=14 alias=retained withdrawal=2 replay=2 task-fault=2 reuse=2".to_owned(),
                format!("PASS smp-completion harts={harts} bounded=1 retained=1 retry=1"),
                format!("PASS smp-instructions harts={harts} bounded=1 retained=1 retry=1"),
            ]);
            let writers: Vec<_> = Regex::new(r"PASS nest-writer task=(\d+)")?
                .captures_iter(log)
                .map(|row| row[1].to_owned())
                .collect();
            let valid = writers.len() == 2 && writers[0] != writers[1];
            evidence = json!({"writers": writers, "target_harts": harts});
            valid
        }
        Kind::Flat => {
            markers.extend(
                [
                    "PASS flat-contracts",
                    "PASS flat-oom",
                    "PASS flat-reservation",
                    "PASS flat-cancel",
                    "PASS flat-race",
                ]
                .map(String::from),
            );
            true
        }
    };
    let missing: Vec<_> = markers
        .iter()
        .filter(|marker| !log.contains(marker.as_str()))
        .collect();
    Ok(json!({
        "passed": valid && missing.is_empty()
            && !log.contains("FAIL")
            && !log.contains("[kernel] fatal"),
        "missing": missing,
        "evidence": evidence
    }))
}
