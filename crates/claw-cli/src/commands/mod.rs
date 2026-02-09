pub mod agent;
pub mod branch;
pub mod change;
pub mod checkout;
pub mod daemon;
pub mod diff;
pub mod git_export;
pub mod init;
pub mod integrate;
pub mod intent;
pub mod log;
pub mod patch;
pub mod ship;
pub mod snapshot;
pub mod sync;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize a new claw repository
    Init(init::InitArgs),
    /// Manage intents
    Intent(intent::IntentArgs),
    /// Manage changes
    Change(change::ChangeArgs),
    /// Create and apply patches
    Patch(patch::PatchArgs),
    /// Sync with a remote repository
    Sync(sync::SyncArgs),
    /// Integrate changes (merge)
    Integrate(integrate::IntegrateArgs),
    /// Ship an intent (finalize, produce capsule)
    Ship(ship::ShipArgs),
    /// Manage agent registrations
    Agent(agent::AgentArgs),
    /// Run the sync daemon
    Daemon(daemon::DaemonArgs),
    /// Run the sync daemon (alias for daemon)
    Serve(daemon::DaemonArgs),
    /// Record a snapshot of the working tree
    Snapshot(snapshot::SnapshotArgs),
    /// Switch branches or restore working tree
    Checkout(checkout::CheckoutArgs),
    /// List, create, or delete branches
    Branch(branch::BranchArgs),
    /// Show revision history
    Log(log::LogArgs),
    /// Show changes between trees
    Diff(diff::DiffArgs),
    /// Export to git format
    GitExport(git_export::GitExportArgs),
}

impl Commands {
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            Commands::Init(args) => init::run(args),
            Commands::Intent(args) => intent::run(args),
            Commands::Change(args) => change::run(args),
            Commands::Patch(args) => patch::run(args),
            Commands::Sync(args) => sync::run(args).await,
            Commands::Integrate(args) => integrate::run(args),
            Commands::Ship(args) => ship::run(args),
            Commands::Agent(args) => agent::run(args),
            Commands::Daemon(args) => daemon::run(args).await,
            Commands::Serve(args) => daemon::run(args).await,
            Commands::Snapshot(args) => snapshot::run(args),
            Commands::Checkout(args) => checkout::run(args),
            Commands::Branch(args) => branch::run(args),
            Commands::Log(args) => log::run(args),
            Commands::Diff(args) => diff::run(args),
            Commands::GitExport(args) => git_export::run(args),
        }
    }
}
