use regex::Regex;
use std::{fs, path::Path, process::Command};
use vanllia_verification::*;
pub fn control(body: &str) -> Result<&str> {
    let lines: Vec<_> = body
        .lines()
        .filter(|s| s.starts_with("Room baseline:"))
        .collect();
    require(
        lines.len() == 1,
        "requested comparison needs exactly one 'Room baseline: <full SHA>' line",
    )?;
    let pin = lines[0]
        .strip_prefix("Room baseline: ")
        .ok_or("malformed Room baseline")?;
    require(
        pin.len() == 40
            && pin
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
        "Room baseline must be a full lowercase SHA",
    )?;
    Ok(pin)
}
pub fn baseline(path: &Path) -> Result<()> {
    let event: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
    let body = event["pull_request"]["body"].as_str().unwrap_or("");
    println!("sha={}", control(body)?);
    Ok(())
}
pub fn symbolize(elf: &Path, serial: &Path, addr2line: &str) -> Result<()> {
    let pc = Regex::new(r"\bpc=(0x[0-9a-fA-F]+)")?;
    let bytes = fs::read(serial)?;
    for line in String::from_utf8_lossy(&bytes).lines() {
        if (line.contains("source=Trap") || line.starts_with("frame source="))
            && let Some(m) = pc.captures(line)
        {
            println!("{line}");
            // Literal PC is required: a RISC-V return can follow a 2-byte call.
            let status = Command::new(addr2line)
                .args(["-f", "-C", "-e"])
                .arg(elf)
                .arg(&m[1])
                .status()?;
            require(status.success(), "addr2line failed")?;
        }
    }
    Ok(())
}
