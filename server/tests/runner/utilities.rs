use kernel_verification::{Result, require};
use regex::Regex;
use std::{fs, path::Path, process::Command};

pub fn symbolize(elf: &Path, serial: &Path, addr2line: &str) -> Result<()> {
    let pc = Regex::new(r"\bpc=(0x[0-9a-fA-F]+)")?;
    let bytes = fs::read(serial)?;
    for line in String::from_utf8_lossy(&bytes).lines() {
        if (line.contains("source=Trap") || line.starts_with("frame source="))
            && let Some(m) = pc.captures(line)
        {
            println!("{line}");
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
