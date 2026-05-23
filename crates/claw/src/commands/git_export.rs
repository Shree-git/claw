use clap::Args;
use std::path::{Component, Path};

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_git::exporter::GitExporter;
use claw_store::ClawStore;

use super::git_notes::{write_note, GitProvenanceNote};
use crate::config::find_repo_root;

const GIT_EXPORT_JSON_SCHEMA_VERSION: u8 = 1;

#[derive(Args)]
pub struct GitExportArgs {
    /// Output command results as JSON
    #[arg(long, global = true)]
    json: bool,
    /// Ref to export (default: heads/main)
    #[arg(long, name = "ref", default_value = "heads/main")]
    ref_name: String,
    /// Git branch name to create
    #[arg(long, default_value = "claw/main")]
    branch: String,
    /// Path to .git directory
    #[arg(long, default_value = ".git")]
    git_dir: String,
    /// Export every claw branch under heads/* to git branches
    #[arg(long)]
    all_heads: bool,
    /// Prefix used for git branch names when --all-heads is set
    #[arg(long, default_value = "claw/")]
    branch_prefix: String,
    /// Export Claw provenance into git notes
    #[arg(long)]
    git_notes: bool,
    /// Git notes ref used when --git-notes is enabled
    #[arg(long, default_value = "claw")]
    notes_ref: String,
    /// Preview export plan without writing Git objects, refs, or notes
    #[arg(long)]
    dry_run: bool,
}

fn validate_git_branch_path(branch: &str) -> anyhow::Result<()> {
    let path = Path::new(branch);
    if path.is_absolute() {
        anyhow::bail!("invalid branch name '{}': must be relative", branch);
    }

    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => {
                anyhow::bail!(
                    "invalid branch name '{}': cannot contain '.', '..', or root components",
                    branch
                );
            }
        }
    }

    Ok(())
}

pub fn run(args: GitExportArgs) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let git_dir = root.join(&args.git_dir);
    let git_objects_dir = git_dir.join("objects");
    let mut exporter = GitExporter::new(&store);
    let mut exports = Vec::new();

    if args.all_heads {
        validate_git_branch_prefix(&args.branch_prefix)?;
        let mut heads = store.list_refs("heads/")?;
        heads.sort_by(|a, b| a.0.cmp(&b.0));
        if heads.is_empty() {
            anyhow::bail!("no refs found under heads/");
        }

        let mut exported = 0usize;
        for (ref_name, rev_id) in heads {
            let short = ref_name.strip_prefix("heads/").unwrap_or(&ref_name);
            let branch_name = format!("{}{}", args.branch_prefix, short);
            validate_git_branch_path(&branch_name)?;

            if args.dry_run {
                let revision_count = collect_revision_ids(&store, &rev_id)?.len();
                let note_count = if args.git_notes {
                    count_git_provenance_notes(&store, &rev_id)?
                } else {
                    0
                };
                exports.push(serde_json::json!({
                    "source_ref": ref_name,
                    "revision_id": rev_id.to_hex(),
                    "git_branch": branch_name,
                    "revision_count": revision_count,
                    "git_commit": null,
                    "note_count": note_count,
                }));
                if !args.json {
                    println!(
                        "Dry run: would export {} -> refs/heads/{} ({} revision object(s))",
                        ref_name, branch_name, revision_count
                    );
                    if args.git_notes {
                        println!(
                            "  Would write {note_count} provenance note(s) to refs/notes/{}",
                            args.notes_ref
                        );
                    }
                }
                exported += 1;
                continue;
            }

            let sha1 = exporter.export(&rev_id, &git_objects_dir)?;
            write_git_branch_ref(&git_dir, &branch_name, &sha1)?;
            write_change_refs(&store, &exporter, &rev_id, &git_dir)?;
            let note_count = if args.git_notes {
                let written = write_git_provenance_notes(
                    &store,
                    &exporter,
                    &rev_id,
                    &git_dir,
                    &args.notes_ref,
                )?;
                if written > 0 && !args.json {
                    println!(
                        "  wrote {written} provenance note(s) to refs/notes/{}",
                        args.notes_ref
                    );
                }
                written
            } else {
                0
            };

            let git_commit = hex::encode(sha1);
            exports.push(serde_json::json!({
                "source_ref": ref_name,
                "revision_id": rev_id.to_hex(),
                "git_branch": branch_name,
                "revision_count": collect_revision_ids(&store, &rev_id)?.len(),
                "git_commit": git_commit,
                "note_count": note_count,
            }));
            if !args.json {
                println!(
                    "Exported {} -> refs/heads/{} ({})",
                    ref_name, branch_name, git_commit
                );
            }
            exported += 1;
        }
        if args.json {
            print_json(serde_json::json!({
                "schema_version": GIT_EXPORT_JSON_SCHEMA_VERSION,
                "action": "git-export",
                "dry_run": args.dry_run,
                "git_dir": git_dir.display().to_string(),
                "all_heads": true,
                "export_count": exported,
                "git_notes": args.git_notes,
                "notes_ref": if args.git_notes { Some(args.notes_ref.as_str()) } else { None },
                "exports": exports,
            }))?;
        } else if args.dry_run {
            println!("Dry run: would export {exported} branch(es) to git.");
        } else {
            println!("Exported {exported} branch(es) to git.");
        }
    } else {
        let rev_id = store
            .get_ref(&args.ref_name)?
            .ok_or_else(|| anyhow::anyhow!("ref not found: {}", args.ref_name))?;
        validate_git_branch_path(&args.branch)?;
        if args.dry_run {
            let revision_count = collect_revision_ids(&store, &rev_id)?.len();
            let note_count = if args.git_notes {
                count_git_provenance_notes(&store, &rev_id)?
            } else {
                0
            };
            if args.json {
                print_json(serde_json::json!({
                    "schema_version": GIT_EXPORT_JSON_SCHEMA_VERSION,
                    "action": "git-export",
                    "dry_run": true,
                    "git_dir": git_dir.display().to_string(),
                    "all_heads": false,
                    "export_count": 1,
                    "git_notes": args.git_notes,
                    "notes_ref": if args.git_notes { Some(args.notes_ref.as_str()) } else { None },
                    "exports": [{
                        "source_ref": args.ref_name,
                        "revision_id": rev_id.to_hex(),
                        "git_branch": args.branch,
                        "revision_count": revision_count,
                        "git_commit": null,
                        "note_count": note_count,
                    }],
                }))?;
            } else {
                println!(
                    "Dry run: would export {} -> refs/heads/{} ({} revision object(s))",
                    args.ref_name, args.branch, revision_count
                );
                if args.git_notes {
                    println!(
                        "  Would write {note_count} provenance note(s) to refs/notes/{}",
                        args.notes_ref
                    );
                }
                println!("  Git object writes skipped.");
                println!("  Git ref writes skipped.");
            }
            return Ok(());
        }

        let head_sha1 = exporter.export(&rev_id, &git_objects_dir)?;

        write_git_branch_ref(&git_dir, &args.branch, &head_sha1)?;
        write_change_refs(&store, &exporter, &rev_id, &git_dir)?;
        let note_count = if args.git_notes {
            let written =
                write_git_provenance_notes(&store, &exporter, &rev_id, &git_dir, &args.notes_ref)?;
            if written > 0 && !args.json {
                println!(
                    "  wrote {written} provenance note(s) to refs/notes/{}",
                    args.notes_ref
                );
            }
            written
        } else {
            0
        };

        let git_commit = hex::encode(head_sha1);
        if args.json {
            print_json(serde_json::json!({
                "schema_version": GIT_EXPORT_JSON_SCHEMA_VERSION,
                "action": "git-export",
                "dry_run": false,
                "git_dir": git_dir.display().to_string(),
                "all_heads": false,
                "export_count": 1,
                "git_notes": args.git_notes,
                "notes_ref": if args.git_notes { Some(args.notes_ref.as_str()) } else { None },
                "exports": [{
                    "source_ref": args.ref_name,
                    "revision_id": rev_id.to_hex(),
                    "git_branch": args.branch,
                    "revision_count": collect_revision_ids(&store, &rev_id)?.len(),
                    "git_commit": git_commit,
                    "note_count": note_count,
                }],
            }))?;
        } else {
            println!("Exported to git: refs/heads/{}", args.branch);
            println!("  SHA-1: {git_commit}");
        }
    }

    Ok(())
}

fn print_json(value: serde_json::Value) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn count_git_provenance_notes(store: &ClawStore, start: &ObjectId) -> anyhow::Result<usize> {
    let mut count = 0usize;
    for rev_id in collect_revision_ids(store, start)? {
        let rev_obj = store.load_object(&rev_id)?;
        let Object::Revision(rev) = rev_obj else {
            continue;
        };
        if GitProvenanceNote::from_revision(store, &rev_id, &rev)?.is_some() {
            count += 1;
        }
    }
    Ok(count)
}

fn write_git_branch_ref(
    git_dir: &std::path::Path,
    branch: &str,
    sha1: &[u8; 20],
) -> anyhow::Result<()> {
    let refs_dir = git_dir.join("refs").join("heads");
    std::fs::create_dir_all(&refs_dir)?;
    let branch_path = refs_dir.join(branch);
    write_bytes_atomic(&branch_path, format!("{}\n", hex::encode(sha1)).as_bytes())?;
    Ok(())
}

fn validate_git_branch_prefix(prefix: &str) -> anyhow::Result<()> {
    if prefix.is_empty() {
        return Ok(());
    }
    let normalized = prefix.trim_end_matches('/');
    if normalized.is_empty() {
        return Ok(());
    }
    validate_git_branch_path(normalized)
}

fn write_change_refs(
    store: &ClawStore,
    exporter: &GitExporter,
    start: &ObjectId,
    git_dir: &std::path::Path,
) -> anyhow::Result<()> {
    let refs_dir = git_dir.join("refs").join("claw").join("changes");
    std::fs::create_dir_all(&refs_dir)?;

    for id in collect_revision_ids(store, start)? {
        if let Ok(Object::Revision(ref rev)) = store.load_object(&id) {
            if let (Some(change_id), Some(sha1)) = (rev.change_id.as_ref(), exporter.get_sha1(&id))
            {
                write_bytes_atomic(
                    &refs_dir.join(change_id.to_string()),
                    format!("{}\n", hex::encode(sha1)).as_bytes(),
                )?;
            }
        }
    }

    Ok(())
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent)?;

    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    {
        use std::io::Write;

        let file = temp.as_file_mut();
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    temp.persist(path).map_err(|err| err.error)?;
    if let Ok(parent_dir) = std::fs::File::open(parent) {
        parent_dir.sync_all()?;
    }
    Ok(())
}

fn write_git_provenance_notes(
    store: &ClawStore,
    exporter: &GitExporter,
    start: &ObjectId,
    git_dir: &std::path::Path,
    notes_ref: &str,
) -> anyhow::Result<usize> {
    let mut written = 0usize;
    for rev_id in collect_revision_ids(store, start)? {
        let rev_obj = store.load_object(&rev_id)?;
        let Object::Revision(rev) = rev_obj else {
            continue;
        };
        let Some(note) = GitProvenanceNote::from_revision(store, &rev_id, &rev)? else {
            continue;
        };
        let Some(commit_sha1) = exporter.get_sha1(&rev_id) else {
            continue;
        };
        write_note(git_dir, notes_ref, &hex::encode(commit_sha1), &note)?;
        written += 1;
    }
    Ok(written)
}

fn collect_revision_ids(store: &ClawStore, start: &ObjectId) -> anyhow::Result<Vec<ObjectId>> {
    let mut out = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let mut queue = vec![*start];

    while let Some(id) = queue.pop() {
        if !visited.insert(id) {
            continue;
        }
        if let Ok(Object::Revision(ref rev)) = store.load_object(&id) {
            out.push(id);
            queue.extend_from_slice(&rev.parents);
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{validate_git_branch_path, validate_git_branch_prefix, GitExportArgs};
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: GitExportArgs,
    }

    #[test]
    fn parses_dry_run_flag() {
        let cli = TestCli::parse_from(["claw", "--dry-run"]);

        assert!(cli.args.dry_run);
    }

    #[test]
    fn parses_json_after_subcommand() {
        let cli = TestCli::parse_from(["claw", "--json", "--dry-run"]);

        assert!(cli.args.json);
        assert!(cli.args.dry_run);
    }

    #[test]
    fn allows_relative_branch_paths() {
        assert!(validate_git_branch_path("main").is_ok());
        assert!(validate_git_branch_path("claw/main").is_ok());
    }

    #[test]
    fn rejects_parent_and_root_components() {
        assert!(validate_git_branch_path("../outside").is_err());
        assert!(validate_git_branch_path("claw/../outside").is_err());
        assert!(validate_git_branch_path("/absolute").is_err());
    }

    #[test]
    fn validates_branch_prefix() {
        assert!(validate_git_branch_prefix("claw/").is_ok());
        assert!(validate_git_branch_prefix("").is_ok());
        assert!(validate_git_branch_prefix("../bad/").is_err());
    }
}
