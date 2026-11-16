use crate::{Result, json, require};
use serde::Serialize;
use std::{
    fs,
    path::Path,
    process::{Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

const CLEANUP_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub command: String,
    pub status: String,
    pub code: Option<i32>,
    pub elapsed_seconds: f64,
    pub passed: bool,
}

#[derive(Debug, Serialize)]
struct Cleanup {
    process_group: Option<u32>,
    spawned: bool,
    unexpected_descendants: bool,
    killed: bool,
    reaped: usize,
    settled: bool,
    elapsed_seconds: f64,
    error: Option<String>,
}

impl Outcome {
    pub fn exited(&self) -> bool {
        self.status == "exited"
    }

    pub fn success(&self) -> bool {
        self.exited() && self.code == Some(0)
    }
}

/// Runs one bounded command in its own process group, recording both output
/// streams. On Linux the runner is a child subreaper and rejects successful
/// leaders that leave processes in the group.
pub fn run(command: &mut Command, directory: &Path, timeout: Duration) -> Result<Outcome> {
    fs::create_dir_all(directory)?;
    let mut result = Outcome {
        command: format!("{command:?}"),
        status: "spawn-error".into(),
        code: None,
        elapsed_seconds: 0.0,
        passed: false,
    };
    let result_path = directory.join("result.json");
    let cleanup_path = directory.join("cleanup.json");
    json(&result_path, &result)?;
    command
        .stdin(Stdio::null())
        .stdout(fs::File::create(directory.join("serial.log"))?)
        .stderr(fs::File::create(directory.join("stderr.log"))?);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    #[cfg(target_os = "linux")]
    {
        if let Err(error) = become_child_subreaper() {
            result.status = "cleanup-setup-failed".into();
            json(
                &cleanup_path,
                &cleanup_result(None, false, false, 0, false, Some(error.to_string())),
            )?;
            json(&result_path, &result)?;
            return Err(error.into());
        }
        install_parent_death_signal(command)?;
    }

    let start = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            json(
                &cleanup_path,
                &cleanup_result(None, false, false, 0, true, None),
            )?;
            return Err(error.into());
        }
    };
    let process_group = child.id();
    let mut wait_error = None;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                set_exit(&mut result, status);
                break;
            }
            Ok(None) if start.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                #[cfg(unix)]
                let _ = kill_group(process_group);
                let _ = child.kill();
                if let Err(error) = child.wait() {
                    wait_error = Some(error);
                }
                result.status = "timeout".into();
                break;
            }
            Err(error) => {
                #[cfg(unix)]
                let _ = kill_group(process_group);
                let _ = child.kill();
                let _ = child.wait();
                wait_error = Some(error);
                result.status = "wait-error".into();
                break;
            }
        }
    }

    result.elapsed_seconds = start.elapsed().as_secs_f64();
    let cleanup = cleanup_group(process_group);
    json(&cleanup_path, &cleanup)?;
    if result.success() && (!cleanup.settled || cleanup.unexpected_descendants) {
        result.status = "cleanup-failed".into();
        result.passed = false;
    }
    json(&result_path, &result)?;

    if let Some(error) = wait_error {
        return Err(error.into());
    }
    require(
        cleanup.settled || result.status != "exited",
        format!(
            "child process group did not settle: {}",
            directory.display()
        ),
    )?;
    Ok(result)
}

fn set_exit(result: &mut Outcome, status: ExitStatus) {
    result.code = status.code();
    result.status = if status.code().is_some() {
        "exited"
    } else {
        "signal"
    }
    .into();
}

fn cleanup_result(
    process_group: Option<u32>,
    spawned: bool,
    unexpected_descendants: bool,
    reaped: usize,
    settled: bool,
    error: Option<String>,
) -> Cleanup {
    Cleanup {
        process_group,
        spawned,
        unexpected_descendants,
        killed: false,
        reaped,
        settled,
        elapsed_seconds: 0.0,
        error,
    }
}

#[cfg(target_os = "linux")]
fn become_child_subreaper() -> std::io::Result<()> {
    if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn install_parent_death_signal(command: &mut Command) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let parent = unsafe { libc::getpid() };
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "runner exited before child start",
                ));
            }
            Ok(())
        });
    }
    Ok(())
}

#[cfg(unix)]
fn kill_group(group: u32) -> bool {
    if group <= 1 {
        return false;
    }
    let group = group as libc::pid_t;
    unsafe { libc::kill(-group, libc::SIGKILL) == 0 }
}

#[cfg(target_os = "linux")]
fn group_exists(group: libc::pid_t) -> bool {
    if group <= 1 {
        return false;
    }
    if unsafe { libc::kill(-group, 0) } == 0 {
        true
    } else {
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

#[cfg(target_os = "linux")]
fn reap_group(group: libc::pid_t) -> Result<usize> {
    require(group > 1, "refusing to reap an invalid process group")?;
    let mut reaped = 0;
    loop {
        let mut status = 0;
        let result = unsafe { libc::waitpid(-group, &mut status, libc::WNOHANG) };
        if result > 0 {
            reaped += 1;
            continue;
        }
        if result == 0 {
            break;
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::EINTR) {
            continue;
        }
        if error.raw_os_error() == Some(libc::ECHILD) {
            break;
        }
        return Err(error.into());
    }
    Ok(reaped)
}

#[cfg(target_os = "linux")]
fn cleanup_group(process_group: u32) -> Cleanup {
    if process_group <= 1 {
        return cleanup_result(
            Some(process_group),
            true,
            false,
            0,
            false,
            Some("invalid process-group ID".into()),
        );
    }

    let started = Instant::now();
    let group = process_group as libc::pid_t;
    let unexpected_descendants = group_exists(group);
    let killed = unexpected_descendants && kill_group(process_group);
    let deadline = Instant::now() + CLEANUP_TIMEOUT;
    let mut reaped = 0;
    let mut error = None;
    while group_exists(group) && Instant::now() < deadline {
        match reap_group(group) {
            Ok(count) => reaped += count,
            Err(reap_error) => {
                error = Some(reap_error.to_string());
                break;
            }
        }
        if group_exists(group) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let settled = !group_exists(group);
    if !settled && error.is_none() {
        error = Some("process group did not settle within 1 second".into());
    }
    if unexpected_descendants && error.is_none() && !killed {
        error = Some("failed to signal an unexpected process-group member".into());
    }
    Cleanup {
        process_group: Some(process_group),
        spawned: true,
        unexpected_descendants,
        killed,
        reaped,
        settled,
        elapsed_seconds: started.elapsed().as_secs_f64(),
        error,
    }
}

#[cfg(not(target_os = "linux"))]
fn cleanup_group(process_group: u32) -> Cleanup {
    Cleanup {
        process_group: Some(process_group),
        spawned: true,
        unexpected_descendants: false,
        killed: false,
        reaped: 0,
        settled: true,
        elapsed_seconds: 0.0,
        error: Some("process-group settlement is only verified on Linux".into()),
    }
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
