use regex::Regex;
use serde_json::{Value, json};
use vanllia_verification::Result;
#[derive(Clone, Debug)]
pub enum Kind {
    Smp,
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
    let mut markers = vec!["task: all tasks exited".to_owned()];
    let mut extra = true;
    let mut evidence = json!({});
    match kind {
        Kind::Smp => {
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
            extra = writers.len() == 2
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
                "irq"=>markers.extend([format!("PASS irq rounds={events}"),"observers=6".into(),"revoke-sign=1 revoke-view=1 ancestor-view=1 rebind=4".into()]),
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
