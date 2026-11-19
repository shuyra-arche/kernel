#![allow(dead_code)]

#[path = "../composition/mod.rs"]
mod composition;
#[path = "../scenario.rs"]
mod scenario;
#[path = "../utilities.rs"]
mod utilities;
#[path = "../verify.rs"]
mod verify;

#[test]
fn only_current_scenarios_are_admitted() {
    use clap::Parser;
    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        command: scenario::Commands,
    }
    for name in ["nest", "flat"] {
        let args = vec![
            "verify",
            name,
            "--kernel",
            "/example/kernel",
            "--output",
            "/example/evidence",
        ];
        assert!(Cli::try_parse_from(args.clone()).is_ok(), "{name}");
        let mut old = args;
        old.extend(["--fixture", "/example/vanllia"]);
        assert!(Cli::try_parse_from(old).is_err());
    }
    for obsolete in [
        "smp",
        "lifecycle",
        "retirement",
        "shutdown",
        "activity",
        "room",
    ] {
        assert!(
            Cli::try_parse_from(["verify", obsolete]).is_err(),
            "{obsolete}"
        );
    }
}

#[test]
fn composition_rejects_unreviewed_source_before_editing() {
    let root = std::env::temp_dir().join(format!("kernel-nest-preimage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let evidence = root.join("evidence");
    std::fs::create_dir_all(&evidence).unwrap();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let paths = [
        "server/src/entry/boot/mod.rs",
        "server/src/entry/report/shutdown.rs",
        "server/src/entry/trap/mod.rs",
        "server/src/metal/riscv/mmu/internal/completion.rs",
    ];
    for path in paths {
        std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
        std::fs::copy(source.join(path), root.join(path)).unwrap();
    }
    let original = std::fs::read(root.join(paths[0])).unwrap();
    std::fs::write(root.join(paths[1]), "unreviewed source").unwrap();
    assert!(composition::apply("nest-provider", &root, &evidence).is_err());
    assert_eq!(std::fs::read(root.join(paths[0])).unwrap(), original);
    std::fs::copy(source.join(paths[1]), root.join(paths[1])).unwrap();
    composition::apply("nest-provider", &root, &evidence).unwrap();
    assert_ne!(std::fs::read(root.join(paths[0])).unwrap(), original);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn nest_requires_completion_and_distinct_writers() {
    let log = "task: all tasks exited\nPASS nest-completion abi=14 alias=retained withdrawal=2 replay=2 task-fault=2 reuse=2\nPASS smp-completion harts=1 bounded=1 retained=1 retry=1\nPASS smp-instructions harts=1 bounded=1 retained=1 retry=1\nPASS nest-writer task=1 delete=false\nPASS nest-writer task=2 delete=true\n";
    assert_eq!(
        verify::check(log, &verify::Kind::Nest, 1).unwrap()["passed"],
        true
    );
    assert_eq!(
        verify::check(&log.replace("task=2", "task=1"), &verify::Kind::Nest, 1).unwrap()["passed"],
        false
    );
    assert_eq!(
        verify::check(
            &log.replace("PASS smp-instructions", "SKIP smp-instructions"),
            &verify::Kind::Nest,
            1
        )
        .unwrap()["passed"],
        false
    );
}

#[test]
fn flat_requires_all_contracts_and_rejects_guest_failure() {
    let log = "PASS flat-contracts\nPASS flat-oom\nPASS flat-reservation\nPASS flat-cancel\nPASS flat-race\ntask: all tasks exited";
    assert_eq!(
        verify::check(log, &verify::Kind::Flat, 1).unwrap()["passed"],
        true
    );
    assert_eq!(
        verify::check(&format!("{log}\nFAIL"), &verify::Kind::Flat, 1).unwrap()["passed"],
        false
    );
    assert_eq!(
        verify::check(&log.replace("PASS flat-race", ""), &verify::Kind::Flat, 1).unwrap()["passed"],
        false
    );
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
