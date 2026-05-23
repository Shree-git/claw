use clap::Args;
use std::path::Path;

use claw_git::importer::{list_git_refs, GitImporter};
use claw_store::ClawStore;

use super::git_notes::{import_note_into_store, read_note};
use crate::config::find_repo_root;

const GIT_IMPORT_JSON_SCHEMA_VERSION: u8 = 1;

#[derive(Args)]
pub struct GitImportArgs {
    /// Output command results as JSON
    #[arg(long, global = true)]
    json: bool,
    /// Git ref to import (e.g. refs/heads/main)
    #[arg(long, default_value = "refs/heads/main")]
    git_ref: String,
    /// Destination claw ref (e.g. heads/main)
    #[arg(long, name = "ref", default_value = "heads/main")]
    ref_name: String,
    /// Path to .git directory
    #[arg(long, default_value = ".git")]
    git_dir: String,
    /// Import every git branch under refs/heads/*
    #[arg(long)]
    all_branches: bool,
    /// Destination claw ref prefix used with --all-branches
    #[arg(long, default_value = "heads/")]
    head_prefix: String,
    /// Import Claw provenance from git notes
    #[arg(long)]
    read_notes: bool,
    /// Git notes ref used when --read-notes is enabled
    #[arg(long, default_value = "claw")]
    notes_ref: String,
    /// Preview refs that would be imported without writing Claw objects or refs
    #[arg(long)]
    dry_run: bool,
}

fn validate_ref_path(ref_name: &str) -> anyhow::Result<()> {
    let path = Path::new(ref_name);
    if path.is_absolute() {
        anyhow::bail!("invalid ref '{}': must be relative", ref_name);
    }

    for component in path.components() {
        match component {
            std::path::Component::Normal(_) => {}
            std::path::Component::CurDir
            | std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                anyhow::bail!(
                    "invalid ref '{}': cannot contain '.', '..', or root components",
                    ref_name
                );
            }
        }
    }

    Ok(())
}

pub fn run(args: GitImportArgs) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let git_dir = root.join(&args.git_dir);
    let mut imports = Vec::new();

    if args.all_branches {
        let prefix = normalize_head_prefix(&args.head_prefix);
        validate_ref_path(prefix.trim_end_matches('/'))?;

        let refs = list_git_refs(&git_dir, "refs/heads/")?;
        if refs.is_empty() {
            anyhow::bail!("no git branches found under refs/heads/");
        }

        let mut importer = if args.dry_run {
            None
        } else {
            Some(GitImporter::new(&store))
        };
        let mut imported = 0usize;
        for (git_ref, sha) in refs {
            let short = git_ref.strip_prefix("refs/heads/").unwrap_or(&git_ref);
            let claw_ref = format!("{prefix}{short}");
            validate_ref_path(&claw_ref)?;
            if args.dry_run {
                imports.push(serde_json::json!({
                    "git_ref": git_ref,
                    "git_commit": hex::encode(sha),
                    "claw_ref": claw_ref,
                    "revision_id": null,
                }));
                if !args.json {
                    println!("Dry run: would import {git_ref} -> {claw_ref}");
                }
                imported += 1;
                continue;
            }
            let importer = importer
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("internal git importer state missing"))?;
            let revision_id = importer.import_ref(&git_dir, &git_ref, &claw_ref)?;
            imports.push(serde_json::json!({
                "git_ref": git_ref,
                "git_commit": hex::encode(sha),
                "claw_ref": claw_ref,
                "revision_id": revision_id.to_hex(),
            }));
            if !args.json {
                println!(
                    "Imported {git_ref} -> {claw_ref} ({})",
                    revision_id.to_hex()
                );
            }
            imported += 1;
        }
        if args.dry_run {
            if args.json {
                print_json(serde_json::json!({
                    "schema_version": GIT_IMPORT_JSON_SCHEMA_VERSION,
                    "action": "git-import",
                    "dry_run": true,
                    "git_dir": git_dir.display().to_string(),
                    "all_branches": true,
                    "import_count": imported,
                    "read_notes": args.read_notes,
                    "notes_ref": if args.read_notes { Some(args.notes_ref.as_str()) } else { None },
                    "notes_imported": null,
                    "imports": imports,
                }))?;
            } else {
                if args.read_notes {
                    println!(
                        "Dry run: would scan refs/notes/{} for provenance notes.",
                        args.notes_ref
                    );
                }
                println!("Dry run: would import {imported} branch(es) from git.");
            }
        } else {
            let imported_notes = if args.read_notes {
                let imported_notes = import_notes_for_imported_commits(
                    &store,
                    importer
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("internal git importer state missing"))?,
                    &git_dir,
                    &args.notes_ref,
                )?;
                if !args.json {
                    println!(
                        "Imported {imported_notes} provenance note(s) from refs/notes/{}",
                        args.notes_ref
                    );
                }
                Some(imported_notes)
            } else {
                None
            };
            if args.json {
                print_json(serde_json::json!({
                    "schema_version": GIT_IMPORT_JSON_SCHEMA_VERSION,
                    "action": "git-import",
                    "dry_run": false,
                    "git_dir": git_dir.display().to_string(),
                    "all_branches": true,
                    "import_count": imported,
                    "read_notes": args.read_notes,
                    "notes_ref": if args.read_notes { Some(args.notes_ref.as_str()) } else { None },
                    "notes_imported": imported_notes,
                    "imports": imports,
                }))?;
            } else {
                println!("Imported {imported} branch(es) from git.");
            }
        }
    } else {
        validate_ref_path(&args.ref_name)?;
        if args.dry_run {
            let (git_ref, sha1) = resolve_git_ref_for_preview(&git_dir, &args.git_ref)?;
            if args.json {
                print_json(serde_json::json!({
                    "schema_version": GIT_IMPORT_JSON_SCHEMA_VERSION,
                    "action": "git-import",
                    "dry_run": true,
                    "git_dir": git_dir.display().to_string(),
                    "all_branches": false,
                    "import_count": 1,
                    "read_notes": args.read_notes,
                    "notes_ref": if args.read_notes { Some(args.notes_ref.as_str()) } else { None },
                    "notes_imported": null,
                    "imports": [{
                        "git_ref": git_ref,
                        "git_commit": hex::encode(sha1),
                        "claw_ref": args.ref_name,
                        "revision_id": null,
                    }],
                }))?;
            } else {
                println!(
                    "Dry run: would import {} ({}) -> {}",
                    git_ref,
                    hex::encode(sha1),
                    args.ref_name
                );
                if args.read_notes {
                    println!(
                        "Dry run: would scan refs/notes/{} for provenance notes.",
                        args.notes_ref
                    );
                }
                println!("  Claw object writes skipped.");
                println!("  Claw ref updates skipped.");
            }
            return Ok(());
        }
        let mut importer = GitImporter::new(&store);
        let revision_id = importer.import_ref(&git_dir, &args.git_ref, &args.ref_name)?;
        let imported_notes = if args.read_notes {
            let imported_notes =
                import_notes_for_imported_commits(&store, &importer, &git_dir, &args.notes_ref)?;
            Some(imported_notes)
        } else {
            None
        };
        if args.json {
            print_json(serde_json::json!({
                "schema_version": GIT_IMPORT_JSON_SCHEMA_VERSION,
                "action": "git-import",
                "dry_run": false,
                "git_dir": git_dir.display().to_string(),
                "all_branches": false,
                "import_count": 1,
                "read_notes": args.read_notes,
                "notes_ref": if args.read_notes { Some(args.notes_ref.as_str()) } else { None },
                "notes_imported": imported_notes,
                "imports": [{
                    "git_ref": args.git_ref,
                    "git_commit": null,
                    "claw_ref": args.ref_name,
                    "revision_id": revision_id.to_hex(),
                }],
            }))?;
        } else {
            println!("Imported git ref {} -> {}", args.git_ref, args.ref_name);
            println!("  Revision: {}", revision_id.to_hex());
            if let Some(imported_notes) = imported_notes {
                println!(
                    "Imported {imported_notes} provenance note(s) from refs/notes/{}",
                    args.notes_ref
                );
            }
        }
    }

    Ok(())
}

fn print_json(value: serde_json::Value) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn resolve_git_ref_for_preview(
    git_dir: &std::path::Path,
    git_ref: &str,
) -> anyhow::Result<(String, [u8; 20])> {
    let refs = list_git_refs(git_dir, "refs/")?;
    let mut candidates = vec![git_ref.to_string()];
    if !git_ref.starts_with("refs/") {
        candidates.push(format!("refs/{git_ref}"));
        candidates.push(format!("refs/heads/{git_ref}"));
    }

    refs.into_iter()
        .find(|(name, _)| candidates.iter().any(|candidate| candidate == name))
        .ok_or_else(|| anyhow::anyhow!("git ref not found: {git_ref}"))
}

fn normalize_head_prefix(prefix: &str) -> String {
    if prefix.is_empty() {
        return "heads/".to_string();
    }
    if prefix.ends_with('/') {
        prefix.to_string()
    } else {
        format!("{prefix}/")
    }
}

fn import_notes_for_imported_commits(
    store: &ClawStore,
    importer: &GitImporter<'_>,
    git_dir: &std::path::Path,
    notes_ref: &str,
) -> anyhow::Result<usize> {
    let mut imported = 0usize;
    for (commit_sha, revision_id) in importer.imported_commits() {
        let commit_hex = hex::encode(commit_sha);
        let Some(note) = read_note(git_dir, notes_ref, &commit_hex)? else {
            continue;
        };
        import_note_into_store(store, &revision_id, note)?;
        imported += 1;
    }
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::{normalize_head_prefix, validate_ref_path, GitImportArgs};
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: GitImportArgs,
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
    fn allows_relative_ref_paths() {
        assert!(validate_ref_path("heads/main").is_ok());
        assert!(validate_ref_path("heads/imported/main").is_ok());
    }

    #[test]
    fn rejects_parent_and_root_components() {
        assert!(validate_ref_path("../outside").is_err());
        assert!(validate_ref_path("heads/../outside").is_err());
        assert!(validate_ref_path("/absolute").is_err());
    }

    #[test]
    fn normalizes_head_prefix() {
        assert_eq!(normalize_head_prefix("heads"), "heads/");
        assert_eq!(
            normalize_head_prefix("imports/branches/"),
            "imports/branches/"
        );
        assert_eq!(normalize_head_prefix(""), "heads/");
    }
}
