#![allow(dead_code)]

#[path = "../composition/mod.rs"]
mod composition;
#[path = "../scenario.rs"]
mod scenario;
#[path = "../suite.rs"]
mod suite;
#[path = "../utilities.rs"]
mod utilities;
#[path = "../verify.rs"]
mod verify;

#[test]
fn projects_own_their_manifests_and_verdicts() {
    use std::{collections::BTreeSet, fs, path::Path};
    fn visit(path: &Path, ids: &mut BTreeSet<String>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.join("suite.json").is_file() {
                let (project, _) = suite::read(&path).unwrap();
                assert!(ids.insert(project.id));
            } else if path.is_dir() {
                visit(&path, ids);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target");
    let mut ids = BTreeSet::new();
    visit(&root, &mut ids);
    assert!(!ids.is_empty());
}

#[test]
fn runner_accepts_project_paths_without_named_commands() {
    use clap::Parser;
    #[derive(Parser)]
    struct Cli {
        #[command(flatten)]
        inputs: scenario::Inputs,
    }
    let args = [
        "verify",
        "--suite",
        "server/tests/target/example",
        "--kernel",
        "/example/kernel",
        "--output",
        "/example/evidence",
    ];
    assert!(Cli::try_parse_from(args).is_ok());
    assert!(
        Cli::try_parse_from(["verify", "--kernel", "/example/kernel"]).is_err()
    );
}

#[test]
fn provider_patches_fail_closed_on_unreviewed_sources() {
    let root = std::env::temp_dir().join(format!("kernel-provider-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let project = root.join("project");
    let provider = root.join("provider");
    let evidence = root.join("evidence");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&provider).unwrap();
    std::fs::create_dir_all(&evidence).unwrap();
    let original = "before\n";
    let source = provider.join("source.rs");
    std::fs::write(&source, original).unwrap();
    let sha = kernel_verification::digest(&source).unwrap();
    std::fs::write(
        project.join("provider.json"),
        serde_json::to_vec(&serde_json::json!({"source.rs":sha})).unwrap(),
    )
    .unwrap();
    std::fs::write(
        project.join("provider.patch"),
        "--- a/source.rs\n+++ b/source.rs\n@@ -1 +1 @@\n-before\n+after\n",
    )
    .unwrap();
    std::fs::write(&source, "changed\n").unwrap();
    assert!(composition::apply(&project, &provider, &evidence).is_err());
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "changed\n");
    std::fs::write(&source, original).unwrap();
    composition::apply(&project, &provider, &evidence).unwrap();
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "after\n");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn generic_verdict_rejects_missing_and_duplicate_evidence() {
    let expected = vec![
        "task: all tasks exited".to_owned(),
        "PASS completed harts={harts}".to_owned(),
    ];
    let distinct = suite::Distinct {
        pattern: r"PASS writer=(\d+)".to_owned(),
        count: 2,
    };
    let log = "task: all tasks exited\nPASS completed harts=1\nPASS writer=3\nPASS writer=7\n";
    assert_eq!(
        verify::check(log, &expected, Some(&distinct), 1).unwrap()["passed"],
        true
    );
    for invalid in [
        log.replace("writer=7", "writer=3"),
        log.replace("harts=1", "harts=4"),
        log.replace("PASS writer=7", "FAIL writer=7"),
    ] {
        assert_eq!(
            verify::check(&invalid, &expected, Some(&distinct), 1).unwrap()["passed"],
            false
        );
    }
}

#[cfg(unix)]
#[test]
fn symbolizer_preserves_literal_return_addresses() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("kernel-symbolize-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir(&root).unwrap();
    let tool = root.join("addr2line");
    let serial = root.join("serial.log");
    std::fs::write(&tool, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(&serial, b"trap source=Trap pc=0x102\n").unwrap();
    utilities::symbolize(&root.join("kernel.elf"), &serial, tool.to_str().unwrap()).unwrap();
    assert!(utilities::symbolize(&root.join("kernel.elf"), &serial, "/bin/false").is_err());
    std::fs::remove_dir_all(root).unwrap();
}

mod execution;
