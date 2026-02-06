pub mod agent;
pub mod change;
pub mod daemon;
pub mod init;
pub mod integrate;
pub mod intent;
pub mod patch;
pub mod ship;
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
        }
    }
}
