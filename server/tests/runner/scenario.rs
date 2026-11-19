use crate::{
    composition,
    suite::{self, Run, Suite},
    verify,
};
use clap::Args;
use kernel_verification::{process, *};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

#[derive(Args)]
pub struct Inputs {
    #[arg(long)]
    suite: PathBuf,
    #[arg(long)]
    kernel: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long, default_value = "debug", value_parser = ["debug", "release"])]
    profile: String,
    #[arg(long, default_value_t = 1)]
    harts: u8,
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=100))]
    rounds: u8,
}

struct Variant {
    root: PathBuf,
    elf: PathBuf,
    image: Option<PathBuf>,
}

fn prepare(args: &Inputs, suite: &Suite, relative: &Path, root: &Path) -> Result<Variant> {
    let Run::Qemu {
        kernel,
        guest,
        imports,
        features,
        environment,
        patch_provider: needs_patch,
        ..
    } = &suite.run
    else {
        return Err("QEMU provider requested for a non-QEMU suite".into());
    };
    fresh(root)?;
    let provider = root.join("provider");
    let source = revision(&args.kernel)?;
    archive(&args.kernel, &source, &provider)?;
    let patches = root.join("patches");
    fs::create_dir(&patches)?;
    let owner = composition::suite();
    composition::apply(&owner.join(relative), &provider, &patches)?;

    for path in std::iter::once(relative.to_path_buf()).chain(imports.iter().map(PathBuf::from)) {
        let destination = provider.join("server/tests").join(&path);
        for file in composition::hashes(&owner.join(&path))?.keys() {
            let file_path = path.join(file);
            composition::copy(&file_path.to_string_lossy(), &destination.join(file))?;
        }
    }

    let manifest = provider.join("server/tests").join(relative).join("Cargo.toml");
    let mut command = Command::new("cargo");
    command
        .arg("build")
        .current_dir(&provider)
        .env("RUSTFLAGS", FLAGS)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_ENCODED_RUSTFLAGS");
    if *needs_patch {
        patch_provider(&mut command, &provider)?;
    }
    if !features.is_empty() {
        command.arg("--features").arg(features.join(","));
    }
    for (name, value) in environment {
        command.env(name, value);
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
    let mut inputs = serde_json::json!({
        "source": source,
        "suite": suite.id,
        "profile": args.profile,
        "tools": tools(&qemu, &firmware)?,
        "suite_sha256": composition::hashes(&owner.join(relative))?,
        "builds": []
    });
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
    let elf = root.join("platform.elf");
    fs::copy(binaries.join(kernel), &elf)?;
    let image = if let Some(guest) = guest {
        fs::copy(binaries.join(&guest.binary), root.join("guest.elf"))?;
        let path = root.join("initrd.bin");
        fs::write(
            &path,
            capsule::initrd(&fs::read(root.join("guest.elf"))?, guest.capsule)?,
        )?;
        Some(path)
    } else {
        None
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
    })
}

fn boot(args: &Inputs, suite: &Suite, variant: &Variant, hart: u8, round: u8) -> Result<bool> {
    let Run::Qemu {
        expects, distinct, ..
    } = &suite.run
    else {
        return Err("non-QEMU suite cannot boot".into());
    };
    let evidence = variant.root.join(format!("harts-{hart}-round-{round}"));
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
        .arg(hart.to_string())
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
    let details = verify::check(&log, expects, distinct.as_ref(), hart)?;
    result.passed = result.success() && details["passed"] == true;
    let mut value = serde_json::to_value(&result)?;
    value["scenario"] = details;
    value["platform_sha256"] = digest(&variant.elf)?.into();
    json(&evidence.join("result.json"), &value)?;
    println!(
        "{} {} {} harts={hart} round={round}: {}",
        if result.passed { "PASS" } else { "FAIL" },
        suite.id,
        args.profile,
        evidence.display()
    );
    Ok(result.passed)
}

fn cargo_test(args: &Inputs, package: &str, test: &str, ignored: bool) -> Result<()> {
    let mut command = Command::new("cargo");
    command
        .args(["+nightly", "test", "--locked", "-p"])
        .arg(package)
        .args(["--test", test, "--target", "x86_64-unknown-linux-gnu"])
        .current_dir(&args.kernel);
    if ignored {
        command.args(["--", "--ignored", "--nocapture"]);
    }
    let result = process::run(
        &mut command,
        &args.output.join("cargo"),
        Duration::from_secs(900),
    )?;
    json(&args.output.join("result.json"), &result)?;
    require(result.success(), "Cargo test suite failed")
}

pub fn run(args: Inputs) -> Result<()> {
    let (suite, relative) = suite::read(&args.suite)?;
    require(
        suite.profiles.contains(&args.profile),
        "profile not supported by this test project",
    )?;
    require(
        suite.harts.contains(&args.harts),
        "Hart count not supported by this test project",
    )?;
    let mut args = args;
    args.kernel = args.kernel.canonicalize()?;
    fresh(&args.output)?;
    args.output = args.output.canonicalize()?;
    match &suite.run {
        Run::CargoTest {
            package,
            test,
            ignored,
        } => cargo_test(&args, package, test, *ignored),
        Run::Qemu { .. } => {
            let variant = prepare(
                &args,
                &suite,
                &relative,
                &args.output.join("candidate"),
            )?;
            let mut passed = true;
            for round in 0..args.rounds {
                passed &= boot(&args, &suite, &variant, args.harts, round)?;
            }
            require(
                passed,
                format!("scenario failed; evidence: {}", args.output.display()),
            )
        }
    }
}
