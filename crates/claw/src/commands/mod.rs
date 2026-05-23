pub mod admin;
pub mod agent;
pub mod auth;
pub mod branch;
pub mod bridge;
pub mod capsule;
pub mod change;
pub mod checkout;
pub mod completion;
pub mod daemon;
pub mod diff;
pub mod doctor;
pub mod evidence;
pub mod git_export;
pub mod git_import;
mod git_notes;
pub mod git_roundtrip;
pub mod init;
pub mod integrate;
pub mod intent;
pub mod log;
pub mod mcp;
pub mod migration;
mod object_refs;
pub mod patch;
pub mod plugin;
pub mod policy;
pub mod provenance;
pub mod remote;
pub mod repair;
mod repo_health;
pub mod resolve;
pub mod review;
pub mod ship;
pub mod show;
pub mod snapshot;
pub mod status;
pub mod story;
pub mod sync;
pub mod timeline;
pub mod trust;
pub mod version;

use clap::Subcommand;

#[derive(Clone, Debug)]
pub enum ErrorFormat {
    Human,
    Json,
}

#[derive(Clone, Debug)]
pub struct RuntimeOptions {
    pub profile: String,
    pub compat_check: bool,
    pub error_format: ErrorFormat,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Administrative operations for production deployments
    Admin(admin::AdminArgs),
    /// Initialize a new claw repository
    Init(init::InitArgs),
    /// Generate shell completion scripts
    #[command(alias = "completion")]
    Completions(completion::CompletionArgs),
    /// Run local CLI and repository diagnostics
    #[command(visible_alias = "diag")]
    Doctor(doctor::DoctorArgs),
    /// Query capsule evidence
    Evidence(evidence::EvidenceArgs),
    /// Inspect provenance capsules
    Capsule(capsule::CapsuleArgs),
    /// Explain why revisions are trustworthy
    Trust(trust::TrustArgs),
    /// Show claw version information
    #[command(visible_alias = "ver")]
    Version(version::VersionArgs),
    /// Manage intents
    #[command(visible_alias = "goal")]
    Intent(intent::IntentArgs),
    /// Manage changes
    #[command(visible_alias = "chg")]
    Change(change::ChangeArgs),
    /// Create and apply patches
    Patch(patch::PatchArgs),
    /// Manage external plugins
    Plugin(plugin::PluginArgs),
    /// Manage policies
    Policy(policy::PolicyArgs),
    /// Replay provenance and attach supply-chain attestations
    Provenance(provenance::ProvenanceArgs),
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
    #[command(visible_alias = "snap")]
    Snapshot(snapshot::SnapshotArgs),
    /// Switch branches or restore working tree
    #[command(visible_alias = "co")]
    Checkout(checkout::CheckoutArgs),
    /// List, create, or delete branches
    #[command(visible_alias = "br")]
    Branch(branch::BranchArgs),
    /// Import hosted Git provider metadata into Claw objects
    Bridge(bridge::BridgeArgs),
    /// Show revision history
    #[command(visible_alias = "lg")]
    Log(log::LogArgs),
    /// Migrate teams from Git into Claw intents, changes, metadata, and policy suggestions
    Migration(migration::MigrationArgs),
    /// Run the Claw MCP server for agent integrations
    Mcp(mcp::McpArgs),
    /// Show changes between trees
    #[command(visible_alias = "d")]
    Diff(diff::DiffArgs),
    /// Export to git format
    GitExport(git_export::GitExportArgs),
    /// Import from git format
    GitImport(git_import::GitImportArgs),
    /// Verify claw -> git -> claw roundtrip integrity
    GitRoundtrip(git_roundtrip::GitRoundtripArgs),
    /// Show working tree status
    #[command(visible_alias = "st")]
    Status(status::StatusArgs),
    /// Show details of an object
    #[command(visible_alias = "cat")]
    Show(show::ShowArgs),
    /// Manage merge conflicts
    Resolve(resolve::ResolveArgs),
    /// Manage remote repositories
    Remote(remote::RemoteArgs),
    /// Plan and apply safe repository repairs
    Repair(repair::RepairArgs),
    /// Review work organized by intent, change, revision, and capsule
    Review(review::ReviewArgs),
    /// Export human-readable intent/change history narratives
    Story(story::StoryArgs),
    /// Debug ref, revision, capsule, and policy timelines
    Timeline(timeline::TimelineArgs),
    /// Authenticate with hosted remote profiles
    Auth(auth::AuthArgs),
}

impl Commands {
    pub async fn run(self, runtime: &RuntimeOptions) -> anyhow::Result<()> {
        match self {
            Commands::Admin(args) => admin::run(args, runtime),
            Commands::Init(args) => init::run(args),
            Commands::Completions(args) => completion::run(args),
            Commands::Doctor(args) => doctor::run(args).await,
            Commands::Evidence(args) => evidence::run(args),
            Commands::Capsule(args) => capsule::run(args),
            Commands::Trust(args) => trust::run(args),
            Commands::Version(args) => version::run(args),
            Commands::Intent(args) => intent::run(args),
            Commands::Change(args) => change::run(args),
            Commands::Patch(args) => patch::run(args),
            Commands::Plugin(args) => plugin::run(args).await,
            Commands::Policy(args) => policy::run(args),
            Commands::Provenance(args) => provenance::run(args),
            Commands::Sync(args) => sync::run(args, runtime).await,
            Commands::Integrate(args) => integrate::run(args),
            Commands::Ship(args) => ship::run(args),
            Commands::Agent(args) => agent::run(args),
            Commands::Daemon(args) => daemon::run(args, runtime).await,
            Commands::Serve(args) => daemon::run(args, runtime).await,
            Commands::Snapshot(args) => snapshot::run(args),
            Commands::Checkout(args) => checkout::run(args),
            Commands::Branch(args) => branch::run(args),
            Commands::Bridge(args) => bridge::run(args).await,
            Commands::Log(args) => log::run(args),
            Commands::Migration(args) => migration::run(args),
            Commands::Mcp(args) => mcp::run(args).await,
            Commands::Diff(args) => diff::run(args),
            Commands::GitExport(args) => git_export::run(args),
            Commands::GitImport(args) => git_import::run(args),
            Commands::GitRoundtrip(args) => git_roundtrip::run(args),
            Commands::Status(args) => status::run(args),
            Commands::Show(args) => show::run(args),
            Commands::Resolve(args) => resolve::run(args),
            Commands::Remote(args) => remote::run(args),
            Commands::Repair(args) => repair::run(args),
            Commands::Review(args) => review::run(args),
            Commands::Story(args) => story::run(args),
            Commands::Timeline(args) => timeline::run(args),
            Commands::Auth(args) => auth::run(args).await,
        }
    }
}
