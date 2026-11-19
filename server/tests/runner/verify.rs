use crate::suite::Distinct;
use kernel_verification::Result;
use regex::Regex;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub fn check(log: &str, expects: &[String], distinct: Option<&Distinct>, harts: u8) -> Result<Value> {
    let missing: Vec<_> = expects
        .iter()
        .map(|marker| marker.replace("{harts}", &harts.to_string()))
        .filter(|marker| !log.contains(marker))
        .collect();
    let mut evidence = json!({});
    let mut valid = true;
    if let Some(rule) = distinct {
        let pattern = Regex::new(&rule.pattern)?;
        let values: Vec<_> = pattern
            .captures_iter(log)
            .filter_map(|capture| capture.get(1).map(|value| value.as_str().to_owned()))
            .collect();
        let unique: BTreeSet<_> = values.iter().collect();
        valid &= values.len() == rule.count && unique.len() == rule.count;
        evidence = json!({"distinct": values});
    }
    Ok(json!({
        "passed": valid
            && missing.is_empty()
            && !log.contains("FAIL")
            && !log.contains("[kernel] fatal"),
        "missing": missing,
        "evidence": evidence
    }))
}
