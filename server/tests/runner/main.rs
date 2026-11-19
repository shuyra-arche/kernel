use clap::{Parser, Subcommand};
use kernel_verification::Result;
use std::path::PathBuf;

mod composition;
mod inventory;
mod scenario;
mod suite;
mod utilities;
mod verify;

#[derive(Parser)]
#[command(about = "Kernel target verification")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run(scenario::Inputs),
    Symbolize {
        elf: PathBuf,
        serial: PathBuf,
        #[arg(long, default_value = "llvm-addr2line")]
        addr2line: String,
    },
    Inventory {
        #[arg(long)]
        kernel: PathBuf,
    },
}

fn main() {
    let result: Result<()> = match Cli::parse().command {
        Commands::Run(inputs) => scenario::run(inputs),
        Commands::Symbolize {
            elf,
            serial,
            addr2line,
        } => utilities::symbolize(&elf, &serial, &addr2line),
        Commands::Inventory { kernel } => inventory::run(&kernel),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
