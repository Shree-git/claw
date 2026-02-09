use clap::{Args, Subcommand};

use claw_core::id::ObjectId;
use claw_store::ClawStore;
use claw_sync::client::SyncClient;
use claw_sync::negotiation::find_reachable_objects;

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
        /// Force non-fast-forward push
        #[arg(long)]
        force: bool,
    },
    /// Pull objects from remote
    Pull {
        /// Remote address
        #[arg(short, long)]
        remote: String,
        /// Ref to pull
        #[arg(short = 'b', long, default_value = "heads/main")]
        ref_name: String,
        /// Force non-fast-forward update
        #[arg(long)]
        force: bool,
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
            force,
        } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let mut client = SyncClient::connect(&remote).await?;

            // Get local ref
            let local_id = store
                .get_ref(&ref_name)?
                .ok_or_else(|| anyhow::anyhow!("ref not found: {ref_name}"))?;

            // Compute reachable objects from local head
            let reachable = find_reachable_objects(&store, &[local_id]);
            let push_ids: Vec<ObjectId> = reachable.into_iter().collect();

            // Push full closure
            let resp = client.push_objects(&store, &push_ids).await?;
            println!("Push: {}", resp.message);

            // Update remote ref with CAS
            let remote_refs = client.advertise_refs("").await?;
            let remote_old = remote_refs
                .iter()
                .find(|(name, _)| name == &ref_name)
                .map(|(_, id)| *id);

            let updates = vec![(ref_name.clone(), remote_old, local_id)];
            let ref_resp = client.update_refs(&updates, force).await?;

            if ref_resp.success {
                println!("Pushed {} to {}", ref_name, remote);
            } else {
                anyhow::bail!("ref update failed: {}", ref_resp.message);
            }
        }
        SyncCommand::Pull {
            remote,
            ref_name,
            force,
        } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let mut client = SyncClient::connect(&remote).await?;

            let remote_refs = client.advertise_refs("").await?;
            let remote_target = remote_refs
                .iter()
                .find(|(name, _)| name == &ref_name)
                .map(|(_, id)| *id);

            let remote_id = match remote_target {
                Some(id) => id,
                None => {
                    println!("Remote ref {ref_name} not found");
                    return Ok(());
                }
            };

            // Send have set (local ref targets)
            let local_id = store.get_ref(&ref_name)?;
            let have: Vec<ObjectId> = local_id.into_iter().collect();

            let fetched = client.fetch_objects(&store, &[remote_id], &have).await?;
            println!("Fetched {} objects", fetched.len());

            // FF check: if we have a local ref, verify it's an ancestor of the remote
            if let Some(local) = store.get_ref(&ref_name)? {
                let is_ff = claw_sync::ancestry::is_ancestor(&store, &local, &remote_id);
                if !is_ff && !force {
                    anyhow::bail!(
                        "non-fast-forward update on {}; use --force to override",
                        ref_name
                    );
                }
            }

            // Update local ref via CAS
            let old = store.get_ref(&ref_name)?;
            store.update_ref_cas(&ref_name, old.as_ref(), &remote_id, "sync", "pull")?;
            println!("Updated {} to {}", ref_name, remote_id);
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
