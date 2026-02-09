use clap::Parser;
use tracing_subscriber::EnvFilter;

mod commands;
mod config;
mod conflict_writer;
mod diff_render;
mod error;
mod ignore;
mod merge_state;
mod output;
mod worktree;

use commands::Commands;

#[derive(Parser)]
#[command(name = "claw", version, about = "Intent-native, agent-native version control")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    cli.command.run().await
}
