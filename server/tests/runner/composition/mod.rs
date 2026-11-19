//! Reviewed source patches for a test's disposable Kernel provider.
use kernel_verification::*;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn suite() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned()
}

pub fn apply(project: &Path, root: &Path, evidence: &Path) -> Result<()> {
    let contract = project.join("provider.json");
    let patch = project.join("provider.patch");
    let inputs: BTreeMap<String, String> = serde_json::from_slice(&fs::read(&contract)?)?;
    require(!inputs.is_empty(), "empty provider patch contract")?;
    for (path, expected) in &inputs {
        require(
            digest(&root.join(path))? == *expected,
            format!("provider preimage changed: {path}; review the test patch"),
        )?;
    }
    for check in [true, false] {
        let mut command = Command::new("git");
        command
            .current_dir(root)
            .env("GIT_CEILING_DIRECTORIES", root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .args(["apply", "--unsafe-paths"])
            .arg(format!("--directory={}", root.display()));
        if check {
            command.arg("--check");
        }
        let output = command.arg(&patch).output()?;
        require(
            output.status.success(),
            format!(
                "provider patch: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        )?;
    }
    fs::copy(&patch, evidence.join("provider.patch"))?;
    fs::copy(&contract, evidence.join("provider.json"))?;
    Ok(())
}

pub fn copy(relative: &str, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(suite().join(relative), destination)?;
    Ok(())
}

pub fn hashes(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut result = BTreeMap::new();
    fn visit(root: &Path, path: &Path, result: &mut BTreeMap<String, String>) -> Result<()> {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_name() == ".git"
                || (entry.file_name() == "target" && path != suite().join("target"))
            {
                continue;
            }
            if path.is_dir() {
                visit(root, &path, result)?;
            } else if path.is_file() {
                result.insert(
                    path.strip_prefix(root)?.display().to_string(),
                    digest(&path)?,
                );
            }
        }
        Ok(())
    }
    visit(root, root, &mut result)?;
    Ok(result)
}
