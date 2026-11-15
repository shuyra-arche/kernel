//! rustc checks production API admission and lifetimes in Debug and Release.
#[test]
fn production_typestate_compile_contracts() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("sync/ui");
    let scratch = std::env::temp_dir().join(format!("kernel-sync-ui-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let mut paths = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .filter(|p| p.file_name().is_some_and(|x| x != "mod.rs"))
        .collect::<Vec<_>>();
    paths.sort();
    for assertions in ["yes", "no"] {
        for path in &paths {
            let source = std::fs::read_to_string(path).unwrap();
            let expected = source.lines().next().unwrap().strip_prefix("// expect: ");
            let output = std::process::Command::new(
                std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()),
            )
            .args([
                "--edition=2024",
                "--emit=metadata",
                "--crate-name=sync_contract",
                "-C",
            ])
            .arg(format!("debug-assertions={assertions}"))
            .arg(path)
            .arg("-o")
            .arg(scratch.join("case.rmeta"))
            .output()
            .unwrap();
            let diagnostic = String::from_utf8_lossy(&output.stderr);
            if let Some(expected) = expected {
                assert!(
                    !output.status.success() && diagnostic.contains(expected),
                    "{} debug={assertions}: expected {expected}, got {diagnostic}",
                    path.display()
                );
            } else {
                assert!(
                    output.status.success(),
                    "{} debug={assertions}: {diagnostic}",
                    path.display()
                );
            }
        }
    }
    std::fs::remove_dir_all(scratch).unwrap();
}
