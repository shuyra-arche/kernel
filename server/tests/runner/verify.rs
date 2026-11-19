use kernel_verification::Result;
use regex::Regex;
use serde_json::{Value, json};
#[derive(Clone, Debug)]
pub enum Kind {
    Nest,
    Smp,
    Shutdown {
        quiet: bool,
    },
    Lifecycle {
        cancel: bool,
        pressure: bool,
    },
    Retirement {
        cancel: bool,
    },
    Activity {
        unsupported: bool,
        stopped: bool,
    },
    Flat {
        timing: bool,
    },
    Room {
        mode: String,
        events: usize,
        latency: bool,
        cold: bool,
    },
}
fn captures(log: &str, pattern: &str) -> Result<Vec<String>> {
    Ok(Regex::new(pattern)?
        .captures_iter(log)
        .map(|c| c[1].to_owned())
        .collect())
}
pub fn check(log: &str, kind: &Kind, harts: u8) -> Result<Value> {
    let mut markers = if matches!(kind, Kind::Shutdown { .. }) {
        vec!["PASS shutdown required=1 prepared=1 converged=1".to_owned()]
    } else {
        vec!["task: all tasks exited".to_owned()]
    };
    let mut extra = true;
    let mut evidence = json!({});
    match kind {
        Kind::Nest => {
            markers.extend([
                "PASS nest-completion abi=14 alias=retained withdrawal=2 replay=2 task-fault=2 reuse=2".to_owned(),
                format!("PASS smp-completion harts={harts} bounded=1 retained=1 retry=1"),
                format!("PASS smp-instructions harts={harts} bounded=1 retained=1 retry=1"),
            ]);
            let writers = captures(log, r"PASS nest-writer task=(\d+)")?;
            extra &= writers.len() == 2 && writers[0] != writers[1];
            evidence = json!({"writers":writers,"target_harts":harts});
        }
        Kind::Retirement { cancel } => {
            let mut result = check(log, &Kind::Smp, harts)?;
            let rows: Vec<_> = Regex::new(r"PASS retirement turns=(\d+) task-records=(\d+) fallback-records=(\d+) cancel=(\d+) pending=0 retained-source=0")?
                .captures_iter(log).map(|row| (1..=4).map(|at| row[at].parse::<usize>()).collect::<std::result::Result<Vec<_>, _>>()).collect::<std::result::Result<_, _>>()?;
            let valid = rows.len() == 1
                && rows[0][0] >= if *cancel { 8 } else { 2 }
                && rows[0][1] > 0
                && (!*cancel || rows[0][2] > 0)
                && rows[0][3] == usize::from(*cancel);
            result["passed"] = json!(result["passed"] == true && valid);
            result["retirement"] = json!({"rows": rows, "cancel": cancel, "passed": valid});
            return Ok(result);
        }
        Kind::Lifecycle { cancel, pressure } => {
            let mut result = check(log, &Kind::Smp, harts)?;
            let turns = captures(log, r"PASS lifecycle turns=(\d+)")?;
            let mut valid = turns.len() == 1
                && turns[0].parse::<usize>()? >= if *cancel { 8 } else { 2 }
                && log.contains(&format!(
                    "cancel={} retained-source=0",
                    usize::from(*cancel)
                ));
            let pressure_rows = Regex::new(
                r"PASS lifecycle-pressure checks=(\d+) denied=(\d+) blocked=(\d+) live=0",
            )?;
            let rows: Vec<_> = pressure_rows.captures_iter(log).collect();
            valid &= if *pressure {
                rows.len() == 1
                    && rows[0][1].parse::<usize>()? > 0
                    && rows[0][1] == rows[0][2]
                    && rows[0][3].parse::<usize>()? > 0
            } else {
                rows.is_empty()
            };
            result["passed"] = json!(result["passed"] == true && valid);
            result["lifecycle"] =
                json!({ "turns": turns, "cancel": cancel, "pressure": pressure, "passed": valid });
            return Ok(result);
        }
        Kind::Smp | Kind::Shutdown { .. } => {
            markers.extend(
                [
                    "PASS native-memory",
                    "PASS native-fault",
                    "PASS acceptance-unit",
                    "PASS acceptance-scopes",
                    "PASS acceptance-lineage",
                    "PASS acceptance-maintenance",
                    "PASS smp-completion",
                    "PASS smp-instructions",
                    "PASS smp-retirement",
                    "PASS smp-executable-rejected",
                ]
                .map(String::from),
            );
            let writers = captures(log, r"PASS smp-writer task=(\d+)")?;
            let migrants = captures(log, r"TRACE smp-migration task=(\d+)")?;
            if let Kind::Shutdown { quiet } = kind {
                extra &= log.contains("task: all tasks exited") != *quiet;
            }
            extra &= writers.len() == 2
                && writers[0] != writers[1]
                && (harts == 1 || writers.iter().all(|w| migrants.contains(w)));
            evidence = json!({"writers":writers,"migrants":migrants});
        }
        Kind::Activity {
            unsupported,
            stopped,
        } => {
            markers.extend(
                if *unsupported {
                    vec!["PASS activity unsupported=1"]
                } else if *stopped {
                    vec![
                        "TRACE activity-observer-stop",
                        "PASS activity observer-stopped=1",
                    ]
                } else {
                    vec![
                        "PASS activity read=1",
                        "PASS activity aliases=3",
                        "PASS activity window=1",
                        "PASS activity capacity=16",
                        "PASS activity writers=3",
                        "PASS activity readonly=1",
                        "PASS activity consumer-exit=1",
                        "PASS activity incoming-budget=32",
                        "PASS activity large-unsupported=1",
                    ]
                }
                .into_iter()
                .map(String::from),
            );
            markers.push(format!("PASS activity-fences harts={harts}"));
            let rows = captures(log, r"TRACE activity-budget ([^\r\n]+)")?;
            let mut metrics = serde_json::Map::new();
            if rows.len() == 1 {
                for c in Regex::new(r"(\w+)=(\d+)")?.captures_iter(&rows[0]) {
                    metrics.insert(c[1].into(), c[2].parse::<u64>()?.into());
                }
            }
            let steps = metrics.get("max_steps").and_then(Value::as_u64);
            extra = rows.len() == 1 && steps.is_some_and(|s| s <= 8);
            if *unsupported {
                extra &= metrics.get("captures").and_then(Value::as_u64) == Some(0);
            } else if !*stopped {
                extra &= steps == Some(8);
            }
            let mut masks = std::collections::BTreeMap::new();
            for c in Regex::new(r"TRACE activity-task id=(\d+) mask=(\d+)")?.captures_iter(log) {
                masks.insert(c[1].to_owned(), c[2].parse::<u64>()?);
            }
            let writers = captures(log, r"TRACE activity-writer task=(\d+)")?;
            let union = writers
                .iter()
                .fold(0u64, |mask, id| mask | masks.get(id).copied().unwrap_or(0));
            if !unsupported && !stopped {
                extra &= writers.len() == 3 && union.count_ones() >= u32::from(harts.min(2));
            }
            evidence = json!({"budget":metrics,"writer_harts":union,"writers":writers});
        }
        Kind::Flat { timing } => {
            markers.push(format!(
                "PASS flat-{}",
                if *timing { "timing" } else { "contracts" }
            ));
            if !timing {
                markers.extend(
                    [
                        "PASS flat-oom",
                        "PASS flat-reservation",
                        "PASS flat-cancel",
                        "PASS flat-race",
                    ]
                    .map(String::from),
                );
            }
        }
        Kind::Room {
            mode,
            events,
            latency,
            cold,
        } => {
            markers.extend(
                [
                    "ROOM:steady",
                    "ROOM:teardown",
                    "iommu: command engine closed",
                ]
                .map(String::from),
            );
            match mode.as_str() {
                "irq"=>markers.extend([format!("PASS irq rounds={events}"),
                        "PASS user-ecall breakpoints=2 unknown=1".into(), "PASS task-start cross-page=1 independent-sp=1 guard=1 retained-stack=1".into(),"observers=6".into(),"revoke-sign=1 revoke-view=1 ancestor-view=1 rebind=4".into()]),
                "room"=>markers.extend(["PASS room workers=8 turns=512 steps=4096".into(),format!("warmup_timers={}",!cold)]),
                "realtime"=>markers.extend([format!("PASS realtime harts={harts}"),if harts==1 {"unsupported=1"} else {"budget=1 replenishment=1 freeze=1 no-credit-reset=1 preemption=1 fixed-hart=1 overload=denied ordinary-pool=restored"}.into()]),
                _=>return Err("invalid Room mode".into()),
            }
            if *latency {
                markers.extend(
                    [
                        "latency-window:",
                        "boundary=whole-interval",
                        "phase=running",
                    ]
                    .map(String::from),
                );
            }
        }
    }
    let missing: Vec<_> = markers
        .iter()
        .filter(|m| !log.contains(m.as_str()))
        .collect();
    Ok(
        json!({"passed":extra && missing.is_empty() && !log.contains("FAIL") && !log.contains("[kernel] fatal"),"missing":missing,"evidence":evidence}),
    )
}
