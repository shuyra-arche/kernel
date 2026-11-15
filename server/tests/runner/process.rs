use crate::{Result, json, require};
use serde::Serialize;
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[derive(Debug, Serialize)]
pub struct Outcome {
    pub command: String,
    pub status: String,
    pub code: Option<i32>,
    pub elapsed_seconds: f64,
    pub passed: bool,
}
impl Outcome {
    pub fn exited(&self) -> bool {
        self.status == "exited"
    }
    pub fn success(&self) -> bool {
        self.exited() && self.code == Some(0)
    }
}
/// Files drain both streams independently. A deadline or signal can never pass,
/// even if a complete success marker was printed before the process died.
pub fn run(command: &mut Command, directory: &Path, timeout: Duration) -> Result<Outcome> {
    fs::create_dir_all(directory)?;
    let mut result = Outcome {
        command: format!("{command:?}"),
        status: "spawn-error".into(),
        code: None,
        elapsed_seconds: 0.0,
        passed: false,
    };
    json(&directory.join("result.json"), &result)?;
    command
        .stdin(Stdio::null())
        .stdout(fs::File::create(directory.join("serial.log"))?)
        .stderr(fs::File::create(directory.join("stderr.log"))?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let start = Instant::now();
    let mut child = command.spawn()?;
    loop {
        if let Some(status) = child.try_wait()? {
            result.code = status.code();
            result.status = if status.code().is_some() {
                "exited"
            } else {
                "signal"
            }
            .into();
            break;
        }
        if start.elapsed() >= timeout {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            child.wait()?;
            result.status = "timeout".into();
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    result.elapsed_seconds = start.elapsed().as_secs_f64();
    json(&directory.join("result.json"), &result)?;
    Ok(result)
}
pub fn build(command: &mut Command, directory: &Path) -> Result<()> {
    let mut result = run(command, directory, Duration::from_secs(900))?;
    result.passed = result.success();
    json(&directory.join("result.json"), &result)?;
    require(
        result.success(),
        format!("build failed: {} ({})", directory.display(), result.status),
    )
}
pub fn accept(result: &mut Outcome, serial: &str, markers: &[String]) -> bool {
    result.passed = result.success()
        && !serial.contains("FAIL")
        && !serial.contains("[kernel] fatal")
        && markers.iter().all(|marker| serial.contains(marker));
    result.passed
}
