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
    output: PathBuf,
    #[arg(long, default_value = "debug", value_parser = ["debug", "release"])]
    profile: String,
    #[arg(long, num_args = 1.., value_parser = clap::value_parser!(u8).range(1..=4))]
    harts: Vec<u8>,
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=100))]
    rounds: u8,
}

#[derive(Subcommand)]
pub enum Commands {
    Nest {
        #[command(flatten)]
        inputs: Inputs,
    },
    Flat {
        #[command(flatten)]
        inputs: Inputs,
    },
}

struct Variant {
    root: PathBuf,
    elf: PathBuf,
    image: Option<PathBuf>,
    kind: Kind,
}

fn prepare(args: &Inputs, kernel: &Path, root: &Path, kind: Kind) -> Result<Variant> {
    fresh(root)?;
    let provider = root.join("provider");
    let source = revision(kernel)?;
    archive(kernel, &source, &provider)?;
    let patches = root.join("patches");
    fs::create_dir(&patches)?;

    let mut build = Command::new("cargo");
    build
        .arg("build")
        .env("RUSTFLAGS", FLAGS)
        .current_dir(&provider);
    for name in [
        "CARGO_TARGET_DIR",
        "CARGO_ENCODED_RUSTFLAGS",
        "FLAT_PROBE_PROVIDER",
    ] {
        build.env_remove(name);
    }

    let (package, patch) = match kind {
        Kind::Nest => ("target/basis/nest/completion", "nest-provider"),
        Kind::Flat => ("target/basis/flat", "flat-provider"),
    };
    compose::apply(patch, &provider, &patches)?;

    let target = provider.join("server/tests").join(package);
    fs::create_dir_all(&target)?;
    for file in compose::hashes(&compose::suite().join(package))?.keys() {
        compose::copy(&format!("{package}/{file}"), &target.join(file))?;
    }

    match kind {
        Kind::Nest => {
            let path = "target/metal/riscv/mmu/completion";
            for file in compose::hashes(&compose::suite().join(path))?.keys() {
                compose::copy(
                    &format!("{path}/{file}"),
                    &provider.join("server/tests").join(path).join(file),
                )?;
            }
            patch_provider(&mut build, &provider)?;
        }
        Kind::Flat => {
            build.args(["--features", "kernel/flat-probe-contracts"]);
            build.env("FLAT_PROBE_PROVIDER", "1");
        }
    }

    let manifest = target.join("Cargo.toml");
    build
        .arg("--manifest-path")
        .arg(&manifest)
        .args(["--target", TARGET]);
    if args.profile == "release" {
        build.arg("--release");
    }

    let qemu = std::env::var("QEMU_RISCV64").unwrap_or("qemu-system-riscv64".into());
    let firmware = Path::new(&std::env::var("QEMU_OPENSBI")?).canonicalize()?;
    let mut inputs = serde_json::json!({
        "source": source,
        "profile": args.profile,
        "scenario": format!("{kind:?}"),
        "tools": tools(&qemu, &firmware)?,
        "suite_sha256": compose::hashes(&compose::suite())?,
        "composition_sha256": compose::hashes(&patches)?,
        "builds": []
    });
    let lock = manifest.with_file_name("Cargo.lock");
    if lock.exists() {
        fs::copy(&lock, root.join("original.Cargo.lock"))?;
    }
    for phase in ["resolve", "locked"] {
        if phase == "locked" {
            build.arg("--locked");
        }
        inputs["builds"]
            .as_array_mut()
            .unwrap()
            .push(format!("{build:?}").into());
        json(&root.join("inputs.json"), &inputs)?;
        process::build(&mut build, &root.join(phase))?;
    }
    fs::copy(&lock, root.join("Cargo.lock"))?;

    let binaries = manifest
        .parent()
        .unwrap()
        .join("target")
        .join(TARGET)
        .join(&args.profile);
    let elf = root.join("platform.elf");
    let image = match kind {
        Kind::Flat => {
            fs::copy(binaries.join("flat-guest"), &elf)?;
            None
        }
        Kind::Nest => {
            fs::copy(binaries.join("memory-platform"), &elf)?;
            let guest = root.join("guest.elf");
            fs::copy(binaries.join("memory-guest"), &guest)?;
            let path = root.join("initrd.bin");
            fs::write(&path, capsule::initrd(&fs::read(&guest)?, 14)?)?;
            Some(path)
        }
    };
    let mut hashes = serde_json::json!({});
    for name in ["platform.elf", "guest.elf", "initrd.bin", "Cargo.lock"] {
        let path = root.join(name);
        if path.exists() {
            hashes[name] = digest(&path)?.into();
        }
    }
    inputs["sha256"] = hashes;
    json(&root.join("inputs.json"), &inputs)?;
    Ok(Variant {
        root: root.to_owned(),
        elf,
        image,
        kind,
    })
}

fn boot(args: &Inputs, variant: &Variant, harts: u8, round: u8) -> Result<bool> {
    let evidence = variant.root.join(format!("harts-{harts}-round-{round}"));
    fresh(&evidence)?;
    let firmware = Path::new(&std::env::var("QEMU_OPENSBI")?).canonicalize()?;
    let qemu = std::env::var("QEMU_RISCV64").unwrap_or("qemu-system-riscv64".into());
    let mut command = Command::new(qemu);
    command
        .args([
            "-accel",
            "tcg,thread=multi",
            "-machine",
            "virt",
            "-cpu",
            "rv64",
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
        .arg(&variant.elf);
    if let Some(image) = &variant.image {
        command.arg("-initrd").arg(image);
    }
    let mut result = process::run(&mut command, &evidence, Duration::from_secs(120))?;
    let log = fs::read_to_string(evidence.join("serial.log"))?;
    let details = verify::check(&log, &variant.kind, harts)?;
    result.passed = result.success() && details["passed"] == true;
    let mut value = serde_json::to_value(&result)?;
    value["scenario"] = details;
    value["platform_sha256"] = digest(&variant.elf)?.into();
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

pub fn run(command: Commands) -> Result<()> {
    let (mut args, kind) = match command {
        Commands::Nest { inputs } => (inputs, Kind::Nest),
        Commands::Flat { inputs } => (inputs, Kind::Flat),
    };
    if args.harts.is_empty() {
        args.harts = vec![1];
    }
    let mut unique = args.harts.clone();
    unique.sort();
    unique.dedup();
    require(
        unique.len() == args.harts.len() && args.harts.iter().all(|h| matches!(h, 1 | 4)),
        "duplicate or unsupported Hart count",
    )?;
    fresh(&args.output)?;
    args.output = args.output.canonicalize()?;
    let variant = prepare(&args, &args.kernel, &args.output.join("candidate"), kind)?;
    let mut order = Vec::new();
    let mut passed = true;
    for hart in &args.harts {
        for round in 0..args.rounds {
            order.push(format!("candidate harts={hart} round={round}"));
            json(&args.output.join("order.json"), &order)?;
            passed &= boot(&args, &variant, *hart, round)?;
        }
    }
    require(
        passed,
        format!("scenario failed; evidence: {}", args.output.display()),
    )
}
