use clap::Args;
use std::path::PathBuf;

use claw_store::ClawStore;

#[derive(Args)]
pub struct InitArgs {
    /// Path to initialize (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
}

pub fn run(args: InitArgs) -> anyhow::Result<()> {
    let path = if args.path.is_absolute() {
        args.path
    } else {
        std::env::current_dir()?.join(&args.path)
    };

    ClawStore::init(&path)?;
    println!("Initialized claw repository at {}", path.display());
    Ok(())
}
