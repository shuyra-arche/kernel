use crate::{
    composition as compose,
    verify::{self, Kind},
};
use clap::{Args, Subcommand};
use kernel_verification::{process, *};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
#[derive(Clone, Args)]
pub struct Inputs {
    #[arg(long)]
    kernel: PathBuf,
    #[arg(long)]
    fixture: Option<PathBuf>,
    #[arg(long)]
    output: PathBuf,
    #[arg(long,default_value="debug",value_parser=["debug","release"])]
    profile: String,
    #[arg(long,num_args=1..,value_parser=clap::value_parser!(u8).range(1..=4))]
    harts: Vec<u8>,
    #[arg(long,default_value_t=1,value_parser=clap::value_parser!(u8).range(1..=100))]
    rounds: u8,
}
#[derive(Subcommand)]
pub enum Commands {
    Smp {
        #[command(flatten)]
        inputs: Inputs,
    },
    Shutdown {
        #[command(flatten)]
        inputs: Inputs,
        #[arg(long)]
        quiet: bool,
    },
    Lifecycle {
        #[command(flatten)]
        inputs: Inputs,
        #[arg(long)]
        cancel: bool,
        #[arg(long)]
        pressure: bool,
    },
    Retirement {
        #[command(flatten)]
        inputs: Inputs,
        #[arg(long)]
        cancel: bool,
    },
    Activity {
        #[command(flatten)]
        inputs: Inputs,
        #[arg(long, conflicts_with = "stopped")]
        unsupported: bool,
        #[arg(long)]
        stopped: bool,
    },
    Flat {
        #[command(flatten)]
        inputs: Inputs,
        #[arg(long,default_value="contracts",value_parser=["contracts","timing"])]
        mode: String,
    },
    Room {
        #[command(flatten)]
        inputs: Inputs,
        #[arg(long,default_value="room",value_parser=["room","irq","realtime"])]
        mode: String,
        #[arg(long,default_value_t=256,value_parser=clap::value_parser!(u16).range(16..=4096))]
        events: u16,
        #[arg(long)]
        baseline: Option<PathBuf>,
        #[arg(long)]
        no_latency: bool,
        #[arg(long)]
        cold_room: bool,
        #[arg(long,conflicts_with_all=["cold_room","baseline"])]
        compare_room_warmup: bool,
    },
}
struct Variant {
    root: PathBuf,
    elf: PathBuf,
    image: Option<PathBuf>,
    kind: Kind,
}
fn copy_guest(file: &str, fixture: &Path) -> Result<()> {
    compose::copy(
        file,
        &fixture
            .join("tests/memory/guest")
            .join(Path::new(file).file_name().unwrap()),
    )
}
fn prepare(args: &Inputs, kernel: &Path, root: &Path, kind: Kind) -> Result<Variant> {
    fresh(root)?;
    let provider = root.join("provider");
    let fixture = root.join("fixture");
    let source = revision(kernel)?;
    // A private guest composition uses the reviewed historical Vanllia source.
    // Flat owns its Guest and uses only the immutable platform Cargo dependency.
    let pin = match args.fixture.as_deref() {
        Some(checkout) => {
            let pin = revision(checkout)?;
            require(
                pin == include_str!("fixture").trim(),
                "unreviewed Vanllia fixture revision",
            )?;
            Some(pin)
        }
        None => None,
    };
    archive(kernel, &source, &provider)?;
    if let (Some(checkout), Some(pin)) = (args.fixture.as_deref(), pin.as_deref()) {
        archive(checkout, pin, &fixture)?;
    }
    let patches = root.join("patches");
    fs::create_dir(&patches)?;
    let legacy_guest = matches!(
        kind,
        Kind::Smp | Kind::Shutdown { .. } | Kind::Lifecycle { .. } | Kind::Retirement { .. }
    );
    if legacy_guest {
        compose::apply("user-fixture", &fixture, &patches)?;
    }
    let manifest;
    let mut command = Command::new("cargo");
    command.arg("build");
    command.env("RUSTFLAGS", FLAGS).current_dir(&provider);
    for name in [
        "CARGO_TARGET_DIR",
        "CARGO_ENCODED_RUSTFLAGS",
        "ACTIVITY_STOPPED",
        "ACTIVITY_UNSUPPORTED",
        "IRQ_EVENTS",
        "ROOM_COLD",
        "FLAT_PROBE_PROVIDER",
    ] {
        command.env_remove(name);
    }
    if legacy_guest {
        compose::copy(
            "target/basis/unit/stack.rs",
            &fixture.join("tests/memory/guest/stack.rs"),
        )?;
    }
    match &kind {
        Kind::Smp | Kind::Lifecycle { .. } | Kind::Retirement { .. } | Kind::Shutdown { .. } => {
            compose::apply("smp-provider", &provider, &patches)?;
            if let Kind::Retirement { cancel } = &kind {
                compose::apply("retirement-provider", &provider, &patches)?;
                if *cancel {
                    compose::apply("retirement-cancel", &provider, &patches)?;
                }
            }
            if let Kind::Lifecycle { cancel, pressure } = &kind {
                compose::apply("lifecycle-provider", &provider, &patches)?;
                if *pressure {
                    compose::apply("lifecycle-pressure", &provider, &patches)?;
                }
                if *cancel {
                    compose::apply("lifecycle-cancel", &provider, &patches)?;
                }
            }
            if let Kind::Shutdown { quiet } = &kind {
                compose::apply("shutdown-provider", &provider, &patches)?;
                if *quiet {
                    compose::apply("shutdown-quiet", &provider, &patches)?;
                }
            }
            compose::apply("smp-fixture", &fixture, &patches)?;
            copy_guest("target/metal/riscv/mmu/completion/cases.rs", &fixture)?;
            fs::rename(
                fixture.join("tests/memory/guest/cases.rs"),
                fixture.join("tests/memory/guest/smp.rs"),
            )?;
            copy_guest("target/metal/riscv/mmu/completion/recovery.rs", &fixture)?;
            manifest = fixture.join("tests/memory/Cargo.toml");
            patch_provider(&mut command, &provider)?;
        }
        Kind::Activity {
            unsupported,
            stopped,
        } => {
            // This is the selected published ABI13 Guest and Platform pair,
            // owned by Kernel rather than the Vanllia product test directory.
            // Keep the historical source preimage checks for Kernel patches.
            compose::apply("activity-pinned-provider", &provider, &patches)?;
            if *stopped {
                compose::apply("activity-stopped", &provider, &patches)?;
                command.env("ACTIVITY_STOPPED", "1");
            }
            if *unsupported {
                command.env("ACTIVITY_UNSUPPORTED", "1");
            }
            let package = "target/basis/nest/memo/activity/pinned";
            let target = provider.join("server/tests").join(package);
            for file in compose::hashes(&compose::suite().join(package))?.keys() {
                compose::copy(&format!("{package}/{file}"), &target.join(file))?;
            }
            manifest = target.join("Cargo.toml");
            patch_provider(&mut command, &provider)?;
        }
        Kind::Flat { timing } => {
            // Apply the same suite to candidate and baseline; never inherit a
            // baseline's older test implementation.
            let target = provider.join("server/tests/target/basis/flat");
            fs::create_dir_all(&target)?;
            for file in compose::hashes(&compose::suite().join("target/basis/flat"))?.keys() {
                compose::copy(&format!("target/basis/flat/{file}"), &target.join(file))?;
            }
            compose::apply("flat-provider", &provider, &patches)?;
            manifest = target.join("Cargo.toml");
            command.args([
                "--features",
                if *timing {
                    "kernel/flat-probe-timing"
                } else {
                    "kernel/flat-probe-contracts"
                },
            ]);
            // Cargo selects the reviewed Platform source from Flat's own
            // immutable Git dependency. No Vanllia test checkout is needed.
            command.env("FLAT_PROBE_PROVIDER", "1");
        }
        Kind::Room {
            mode,
            events,
            latency,
            cold,
        } => {
            // Room, IRQ and Realtime are independent published ABI13 Kernel
            // target Guests, sharing the same reviewed firmware/stack helpers.
            if *latency {
                compose::apply("room-pinned-window", &provider, &patches)?;
                command.args(["--features", "kernel/latency"]);
            }
            let package = match mode.as_str() {
                "room" => "target/basis/room/pinned",
                "irq" => "target/basis/room/irq",
                "realtime" => "target/basis/room/schedule",
                _ => unreachable!("mode checked at CLI admission"),
            };
            if mode == "irq" {
                for name in [
                    "stack.rs",
                    "support.rs",
                    "platform/device.rs",
                    "platform/window.rs",
                ] {
                    let shared = format!("target/basis/room/pinned/{name}");
                    compose::copy(&shared, &provider.join("server/tests").join(&shared))?;
                }
            } else if mode == "realtime" {
                let shared = "target/basis/room/pinned";
                let target = provider.join("server/tests").join(shared);
                for file in compose::hashes(&compose::suite().join(shared))?.keys() {
                    compose::copy(&format!("{shared}/{file}"), &target.join(file))?;
                }
                compose::copy(
                    "target/basis/room/realtime.rs",
                    &provider.join("server/tests/target/basis/room/realtime.rs"),
                )?;
            }
            let target = provider.join("server/tests").join(package);
            for file in compose::hashes(&compose::suite().join(package))?.keys() {
                compose::copy(&format!("{package}/{file}"), &target.join(file))?;
            }
            manifest = target.join("Cargo.toml");
            command.env("IRQ_EVENTS", events.to_string());
            if *cold {
                command.env("ROOM_COLD", "1");
            }
            patch_provider(&mut command, &provider)?;
        }
    }
    // Always take probes from the runner's reviewed suite, including when the
    // selected provider is an earlier baseline without these test files.
    for directory in [
        "target/metal/riscv/mmu/completion",
        "target/basis/nest/memo/activity",
    ] {
        for file in compose::hashes(&compose::suite().join(directory))?.keys() {
            compose::copy(
                &format!("{directory}/{file}"),
                &provider.join("server/tests").join(directory).join(file),
            )?;
        }
    }
    command
        .arg("--manifest-path")
        .arg(&manifest)
        .args(["--target", TARGET]);
    if args.profile == "release" {
        command.arg("--release");
    }
    let qemu = std::env::var("QEMU_RISCV64").unwrap_or("qemu-system-riscv64".into());
    let firmware = Path::new(&std::env::var("QEMU_OPENSBI")?).canonicalize()?;
    let mut inputs = serde_json::json!({"source":source,"fixture":pin,"profile":args.profile,"scenario":format!("{kind:?}"),"tools":tools(&qemu,&firmware)?,
        "suite_sha256":compose::hashes(&compose::suite())?,"composition_sha256":compose::hashes(&patches)?,"builds":[]});
    let lock = manifest.with_file_name("Cargo.lock");
    if lock.exists() {
        fs::copy(&lock, root.join("original.Cargo.lock"))?;
    }
    for phase in ["resolve", "locked"] {
        if phase == "locked" {
            command.arg("--locked");
        }
        inputs["builds"]
            .as_array_mut()
            .unwrap()
            .push(format!("{command:?}").into());
        json(&root.join("inputs.json"), &inputs)?;
        process::build(&mut command, &root.join(phase))?;
    }
    fs::copy(&lock, root.join("Cargo.lock"))?;
    let binaries = manifest
        .parent()
        .unwrap()
        .join("target")
        .join(TARGET)
        .join(&args.profile);
    let is_flat = matches!(kind, Kind::Flat { .. });
    let elf = root.join("platform.elf");
    fs::copy(
        binaries.join(if is_flat {
            "flat-guest"
        } else {
            "memory-platform"
        }),
        &elf,
    )?;
    let image = if is_flat {
        None
    } else {
        fs::copy(binaries.join("memory-guest"), root.join("guest.elf"))?;
        let path = root.join("initrd.bin");
        fs::write(
            &path,
            capsule::initrd(&fs::read(root.join("guest.elf"))?, 13)?,
        )?;
        Some(path)
    };
    for name in ["platform.elf", "guest.elf", "initrd.bin", "Cargo.lock"] {
        let path = root.join(name);
        if path.exists() {
            inputs["sha256"][name] = digest(&path)?.into();
        }
    }
    json(&root.join("inputs.json"), &inputs)?;
    Ok(Variant {
        root: root.to_owned(),
        elf,
        image,
        kind,
    })
}
fn boot(
    args: &Inputs,
    variant: &Variant,
    harts: u8,
    round: u8,
    shared: Option<&Path>,
) -> Result<bool> {
    let evidence = variant.root.join(format!("harts-{harts}-round-{round}"));
    let firmware = Path::new(&std::env::var("QEMU_OPENSBI")?).canonicalize()?;
    let qemu = std::env::var("QEMU_RISCV64").unwrap_or("qemu-system-riscv64".into());
    let activity = matches!(variant.kind, Kind::Activity { .. });
    let room = matches!(variant.kind, Kind::Room { .. });
    let unsupported = matches!(
        variant.kind,
        Kind::Activity {
            unsupported: true,
            ..
        }
    );
    let mut command = Command::new(qemu);
    command
        .args([
            "-accel",
            "tcg,thread=multi",
            "-machine",
            if room || activity {
                "virt,iommu-sys=on"
            } else {
                "virt"
            },
            "-cpu",
            if activity {
                if unsupported {
                    "rv64,svade=on,svadu=off"
                } else {
                    "rv64,svade=off,svadu=off"
                }
            } else {
                "rv64"
            },
            "-smp",
        ])
        .arg(harts.to_string())
        .args([
            "-m",
            "128M",
            "-nographic",
            "-monitor",
            "none",
            "-nic",
            "none",
            "-bios",
        ])
        .arg(firmware)
        .arg("-kernel")
        .arg(shared.unwrap_or(&variant.elf));
    if let Some(image) = &variant.image {
        command.arg("-initrd").arg(image);
    }
    fresh(&evidence)?;
    if room {
        let disk = evidence.join("disk.raw");
        fs::write(&disk, vec![0; 1024 * 1024])?;
        command.arg("-drive").arg(format!("file={},format=raw,if=none,id=disk,aio=threads",disk.display()))
            .args(["-device","virtio-blk-pci,drive=disk,disable-legacy=on,iommu_platform=on,ats=off,addr=1,ioeventfd=off,vectors=0"]);
    }
    let mut result = process::run(&mut command, &evidence, Duration::from_secs(120))?;
    let log = fs::read_to_string(evidence.join("serial.log"))?;
    let details = verify::check(&log, &variant.kind, harts)?;
    result.passed = result.success() && details["passed"] == true;
    let mut value = serde_json::to_value(&result)?;
    value["scenario"] = details;
    value["platform_sha256"] = digest(shared.unwrap_or(&variant.elf))?.into();
    value["host_load_end"] = fs::read_to_string("/proc/loadavg").ok().into();
    json(&evidence.join("result.json"), &value)?;
    println!(
        "{} {:?} {} harts={harts} round={round}: {}",
        if result.passed { "PASS" } else { "FAIL" },
        variant.kind,
        args.profile,
        evidence.display()
    );
    Ok(result.passed)
}
pub(crate) fn admit_fixture(kind: &Kind, fixture: Option<&Path>) -> Result<()> {
    if matches!(kind, Kind::Flat { .. } | Kind::Activity { .. } | Kind::Room { .. }) {
        require(
            fixture.is_none(),
            "this Kernel target has a pinned Platform dependency; --fixture is not accepted",
        )
    } else {
        require(fixture.is_some(), "this target scenario requires --fixture")
    }
}

pub fn run(command: Commands) -> Result<()> {
    let (mut args, kind, baseline, warmup) = match command {
        Commands::Smp { inputs } => (inputs, Kind::Smp, None, false),
        Commands::Shutdown { inputs, quiet } => (inputs, Kind::Shutdown { quiet }, None, false),
        Commands::Lifecycle {
            inputs,
            cancel,
            pressure,
        } => (inputs, Kind::Lifecycle { cancel, pressure }, None, false),
        Commands::Retirement { inputs, cancel } => {
            (inputs, Kind::Retirement { cancel }, None, false)
        }
        Commands::Activity {
            inputs,
            unsupported,
            stopped,
        } => (
            inputs,
            Kind::Activity {
                unsupported,
                stopped,
            },
            None,
            false,
        ),
        Commands::Flat { inputs, mode } => (
            inputs,
            Kind::Flat {
                timing: mode == "timing",
            },
            None,
            false,
        ),
        Commands::Room {
            inputs,
            mode,
            events,
            baseline,
            no_latency,
            cold_room,
            compare_room_warmup,
        } => {
            require(
                mode == "room" || (!cold_room && !compare_room_warmup),
                "cold/warm policy requires --mode room",
            )?;
            (
                inputs,
                Kind::Room {
                    mode,
                    events: usize::from(events),
                    latency: !no_latency,
                    cold: cold_room,
                },
                baseline,
                compare_room_warmup,
            )
        }
    };
    admit_fixture(&kind, args.fixture.as_deref())?;
    if args.harts.is_empty() {
        args.harts = if matches!(kind, Kind::Smp) {
            vec![4]
        } else {
            vec![1, 4]
        };
    }
    let mut unique = args.harts.clone();
    unique.sort();
    unique.dedup();
    require(
        unique.len() == args.harts.len()
            && args
                .harts
                .iter()
                .all(|h| matches!(h, 1 | 2 | 4) && (*h != 2 || matches!(kind, Kind::Smp))),
        "duplicate or unsupported Hart count",
    )?;
    fresh(&args.output)?;
    args.output = args.output.canonicalize()?;
    let mut variants = Vec::new();
    if warmup {
        for (name, cold) in [("cold", true), ("warm", false)] {
            let mut choice = kind.clone();
            if let Kind::Room { cold: value, .. } = &mut choice {
                *value = cold;
            }
            variants.push(prepare(
                &args,
                &args.kernel,
                &args.output.join(name),
                choice,
            )?);
        }
    } else {
        if let Some(base) = baseline {
            variants.push(prepare(
                &args,
                &base,
                &args.output.join("baseline"),
                kind.clone(),
            )?);
        }
        variants.push(prepare(
            &args,
            &args.kernel,
            &args.output.join("candidate"),
            kind,
        )?);
    }
    let shared = if warmup {
        Some(variants[0].elf.as_path())
    } else {
        None
    };
    let mut order = Vec::new();
    let mut passed = true;
    for hart in &args.harts {
        for round in 0..args.rounds {
            let indices: Vec<_> = if round % 2 == 0 {
                (0..variants.len()).collect()
            } else {
                (0..variants.len()).rev().collect()
            };
            for index in indices {
                let variant = &variants[index];
                order.push(format!(
                    "{} harts={hart} round={round}",
                    variant.root.display()
                ));
                json(&args.output.join("order.json"), &order)?;
                passed &= boot(&args, variant, *hart, round, shared)?;
            }
        }
    }
    require(
        passed,
        format!("scenario failed; evidence: {}", args.output.display()),
    )
}
