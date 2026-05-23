use clap::Args;
use std::path::PathBuf;

use claw_store::ClawStore;

#[derive(Args)]
pub struct InitArgs {
    /// Path to initialize (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
    /// Output result as JSON
    #[arg(long)]
    json: bool,
    /// Preview initialization without writing .claw
    #[arg(long)]
    dry_run: bool,
}

pub fn run(args: InitArgs) -> anyhow::Result<()> {
    let path = if args.path.is_absolute() {
        args.path
    } else {
        std::env::current_dir()?.join(&args.path)
    };

    if path.join(".claw").exists() {
        anyhow::bail!(
            "claw repository already exists at {}. Run `claw status` to inspect it.",
            path.display()
        );
    }

    let next_steps = [
        "claw status",
        "claw snapshot -m \"initial snapshot\"",
        "claw intent create --title \"describe the next change\"",
    ];
    let claw_dir = path.join(".claw");

    if args.dry_run {
        if args.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "action": "init",
                    "initialized": false,
                    "path": path.display().to_string(),
                    "claw_dir": claw_dir.display().to_string(),
                    "dry_run": true,
                    "already_initialized": false,
                    "created": false,
                    "head": "heads/main",
                    "next_steps": next_steps,
                }))?
            );
        } else {
            println!("Would initialize claw repository at {}", path.display());
            println!("Initial HEAD: heads/main");
        }
        return Ok(());
    }

    ClawStore::init(&path)?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "action": "init",
                "initialized": true,
                "path": path.display().to_string(),
                "claw_dir": claw_dir.display().to_string(),
                "dry_run": false,
                "already_initialized": false,
                "created": true,
                "head": "heads/main",
                "next_steps": next_steps,
            }))?
        );
    } else {
        println!("Initialized claw repository at {}", path.display());
        println!();
        println!("Next steps:");
        for step in next_steps {
            println!("  {step}");
        }
    }

    Ok(())
}
