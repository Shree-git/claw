use clap::{Args, Subcommand};

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_store::{ClawStore, HeadState};
use claw_sync::client::SyncClient;
use claw_sync::negotiation::find_reachable_objects;
use claw_sync::transport::RemoteTransportConfig;

use crate::auth_store;
use crate::config::find_repo_root;
use crate::worktree;

use super::remote;

#[derive(Args)]
pub struct SyncArgs {
    #[command(subcommand)]
    command: SyncCommand,
}

#[derive(Subcommand)]
enum SyncCommand {
    /// Push objects to remote
    Push {
        /// Remote name or address (e.g., origin or http://localhost:50051)
        #[arg(short, long, default_value = "origin")]
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
        /// Remote name or address
        #[arg(short, long, default_value = "origin")]
        remote: String,
        /// Ref to pull
        #[arg(short = 'b', long, default_value = "heads/main")]
        ref_name: String,
        /// Force non-fast-forward update
        #[arg(long)]
        force: bool,
    },
    /// Clone a remote repository (gRPC URL)
    Clone {
        /// Remote address
        remote: String,
        /// Local path
        #[arg(default_value = ".")]
        path: String,
    },
}

async fn connect_from_remote(
    root: &std::path::Path,
    remote_arg: &str,
) -> anyhow::Result<SyncClient> {
    let resolved = remote::resolve_remote(root, remote_arg)?;
    let transport = match resolved {
        remote::ResolvedRemote::Grpc { addr } => RemoteTransportConfig::Grpc { addr },
        remote::ResolvedRemote::ClawLab {
            base_url,
            repo,
            token_profile,
        } => {
            let profile_name = token_profile
                .clone()
                .unwrap_or_else(|| "default".to_string());
            let profile_name_for_err = profile_name.clone();
            let token = auth_store::resolve_access_token(Some(&profile_name)).ok_or_else(|| {
                anyhow::anyhow!(
                    "no token for profile '{}'; run `claw auth login --profile {}`",
                    profile_name_for_err,
                    profile_name_for_err.clone()
                )
            })?;
            RemoteTransportConfig::Http {
                base_url,
                repo,
                bearer_token: Some(token),
            }
        }
    };

    let client = SyncClient::connect_with_transport(transport).await?;
    Ok(client)
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
            let mut client = connect_from_remote(&root, &remote).await?;

            let local_id = store
                .get_ref(&ref_name)?
                .ok_or_else(|| anyhow::anyhow!("ref not found: {ref_name}"))?;

            let reachable = find_reachable_objects(&store, &[local_id]);
            let push_ids: Vec<ObjectId> = reachable.into_iter().collect();

            let resp = client.push_objects(&store, &push_ids).await?;
            println!("Push: {}", resp.message);

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
            let mut client = connect_from_remote(&root, &remote).await?;

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

            let local_id = store.get_ref(&ref_name)?;
            let have: Vec<ObjectId> = local_id.into_iter().collect();

            let fetched = client.fetch_objects(&store, &[remote_id], &have).await?;
            println!("Fetched {} objects", fetched.len());

            if let Some(local) = store.get_ref(&ref_name)? {
                let is_ff = claw_sync::ancestry::is_ancestor(&store, &local, &remote_id);
                if !is_ff && !force {
                    anyhow::bail!(
                        "non-fast-forward update on {}; use --force to override",
                        ref_name
                    );
                }
            }

            let old = store.get_ref(&ref_name)?;
            store.update_ref_cas(&ref_name, old.as_ref(), &remote_id, "sync", "pull")?;
            println!("Updated {} to {}", ref_name, remote_id);

            let head_state = store.read_head()?;
            if let HeadState::Symbolic {
                ref_name: ref head_ref,
            } = head_state
            {
                if *head_ref == ref_name {
                    let rev_obj = store.load_object(&remote_id)?;
                    if let Object::Revision(ref rev) = rev_obj {
                        if let Some(ref tree_id) = rev.tree {
                            worktree::materialize_tree(&store, tree_id, &root)?;
                            println!("Working tree updated.");
                        }
                    }
                }
            }
        }
        SyncCommand::Clone { remote, path } => {
            let root = std::path::Path::new(&path);
            let store = ClawStore::init(root)?;
            let mut client = SyncClient::connect(&remote).await?;

            let _hello = client.hello().await?;
            let remote_refs = client.advertise_refs("").await?;

            let want: Vec<_> = remote_refs.iter().map(|(_, id)| *id).collect();
            let fetched = client.fetch_objects(&store, &want, &[]).await?;

            for (name, id) in &remote_refs {
                store.set_ref(name, id)?;
            }

            store.write_head(&HeadState::Symbolic {
                ref_name: "heads/main".to_string(),
            })?;

            let main_id = store.get_ref("heads/main")?;
            let checkout_id = main_id.or_else(|| remote_refs.first().map(|(_, id)| *id));
            if let Some(rev_id) = checkout_id {
                let rev_obj = store.load_object(&rev_id)?;
                if let Object::Revision(ref rev) = rev_obj {
                    if let Some(ref tree_id) = rev.tree {
                        worktree::materialize_tree(&store, tree_id, root)?;
                    }
                }
            }

            let config_path = root.join(".claw").join("remotes.toml");
            let mut remotes = remote::load_remotes(&config_path);
            remotes.remotes.insert(
                "origin".to_string(),
                remote::RemoteEntry {
                    kind: Some("grpc".to_string()),
                    url: Some(remote.clone()),
                    ..remote::RemoteEntry::default()
                },
            );
            let content = toml::to_string_pretty(&remotes)?;
            std::fs::write(&config_path, content)?;

            println!(
                "Cloned {} ({} objects, {} refs)",
                remote,
                fetched.len(),
                remote_refs.len()
            );
        }
    }
    Ok(())
}
