use clap::{Args, Subcommand};

use claw_store::ClawStore;
use claw_sync::client::SyncClient;

use crate::config::find_repo_root;

#[derive(Args)]
pub struct SyncArgs {
    #[command(subcommand)]
    command: SyncCommand,
}

#[derive(Subcommand)]
enum SyncCommand {
    /// Push objects to remote
    Push {
        /// Remote address (e.g., http://localhost:50051)
        #[arg(short, long)]
        remote: String,
        /// Ref to push
        #[arg(short = 'b', long, default_value = "heads/main")]
        ref_name: String,
    },
    /// Pull objects from remote
    Pull {
        /// Remote address
        #[arg(short, long)]
        remote: String,
        /// Ref to pull
        #[arg(short = 'b', long, default_value = "heads/main")]
        ref_name: String,
    },
    /// Clone a remote repository
    Clone {
        /// Remote address
        remote: String,
        /// Local path
        #[arg(default_value = ".")]
        path: String,
    },
}

pub async fn run(args: SyncArgs) -> anyhow::Result<()> {
    match args.command {
        SyncCommand::Push {
            remote,
            ref_name,
        } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let mut client = SyncClient::connect(&remote).await?;

            // Get local ref
            let local_id = store
                .get_ref(&ref_name)?
                .ok_or_else(|| anyhow::anyhow!("ref not found: {ref_name}"))?;

            // Collect objects to push (simplified - just push the head object)
            let resp = client.push_objects(&store, &[local_id]).await?;
            println!("Push: {}", resp.message);

            // Update remote ref
            // In a full impl, we'd use update_refs RPC
            println!("Pushed {} to {}", ref_name, remote);
        }
        SyncCommand::Pull {
            remote,
            ref_name,
        } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let mut client = SyncClient::connect(&remote).await?;

            let remote_refs = client.advertise_refs("").await?;
            let want: Vec<_> = remote_refs
                .iter()
                .filter(|(name, _)| name == &ref_name)
                .map(|(_, id)| *id)
                .collect();

            if want.is_empty() {
                println!("Remote ref {ref_name} not found");
                return Ok(());
            }

            let fetched = client.fetch_objects(&store, &want, &[]).await?;
            println!("Fetched {} objects", fetched.len());
        }
        SyncCommand::Clone { remote, path } => {
            let store = ClawStore::init(std::path::Path::new(&path))?;
            let mut client = SyncClient::connect(&remote).await?;

            let _hello = client.hello().await?;
            let remote_refs = client.advertise_refs("").await?;

            let want: Vec<_> = remote_refs.iter().map(|(_, id)| *id).collect();
            let fetched = client.fetch_objects(&store, &want, &[]).await?;

            // Set local refs
            for (name, id) in &remote_refs {
                store.set_ref(name, id)?;
            }

            println!("Cloned {} ({} objects, {} refs)", remote, fetched.len(), remote_refs.len());
        }
    }
    Ok(())
}
