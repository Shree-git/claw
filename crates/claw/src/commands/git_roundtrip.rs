use std::path::{Component, Path};

use clap::Args;

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_core::types::Revision;
use claw_git::exporter::GitExporter;
use claw_git::importer::GitImporter;
use claw_store::ClawStore;

use super::git_notes::{import_note_into_store, read_note, write_note, GitProvenanceNote};
use crate::config::find_repo_root;

const GIT_ROUNDTRIP_JSON_SCHEMA_VERSION: u8 = 1;

#[derive(Args)]
pub struct GitRoundtripArgs {
    /// Output command results as JSON
    #[arg(long, global = true)]
    json: bool,
    /// Source claw ref to verify
    #[arg(long, name = "ref", default_value = "heads/main")]
    ref_name: String,
    /// Path to .git directory
    #[arg(long, default_value = ".git")]
    git_dir: String,
    /// Temporary git branch for roundtrip export
    #[arg(long, default_value = "claw/roundtrip-verify")]
    branch: String,
    /// Destination claw ref for roundtrip import
    #[arg(long, default_value = "heads/roundtrip-verify")]
    import_ref: String,
    /// Include provenance notes in roundtrip check
    #[arg(long)]
    with_notes: bool,
    /// Git notes ref used when --with-notes is set
    #[arg(long, default_value = "claw")]
    notes_ref: String,
}

pub fn run(args: GitRoundtripArgs) -> anyhow::Result<()> {
    validate_relative_ref_path(&args.import_ref)?;
    validate_relative_ref_path(&args.branch)?;

    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let source_revision_id = store
        .get_ref(&args.ref_name)?
        .ok_or_else(|| anyhow::anyhow!("ref not found: {}", args.ref_name))?;

    let git_dir = root.join(&args.git_dir);
    let git_objects_dir = git_dir.join("objects");

    let mut exporter = GitExporter::new(&store);
    let exported_commit_sha = exporter.export(&source_revision_id, &git_objects_dir)?;
    let git_ref = normalize_git_branch_ref(&args.branch);
    write_git_branch_ref(&git_dir, &git_ref, &exported_commit_sha)?;

    if args.with_notes {
        write_provenance_notes(
            &store,
            &exporter,
            &source_revision_id,
            &git_dir,
            &args.notes_ref,
        )?;
    }

    let mut importer = GitImporter::new(&store);
    let imported_revision_id = importer.import_ref(&git_dir, &git_ref, &args.import_ref)?;

    if args.with_notes {
        let mut notes_imported = 0usize;
        for (commit_sha, revision_id) in importer.imported_commits() {
            let Some(note) = read_note(&git_dir, &args.notes_ref, &hex::encode(commit_sha))? else {
                continue;
            };
            import_note_into_store(&store, &revision_id, note)?;
            notes_imported += 1;
        }
        let verification = verify_roundtrip(&store, &source_revision_id, &imported_revision_id)?;
        print_roundtrip_result(
            &args,
            &git_dir,
            &git_ref,
            &exported_commit_sha,
            &source_revision_id,
            &imported_revision_id,
            Some(notes_imported),
            &verification,
        )?;
        return Ok(());
    }

    let verification = verify_roundtrip(&store, &source_revision_id, &imported_revision_id)?;
    print_roundtrip_result(
        &args,
        &git_dir,
        &git_ref,
        &exported_commit_sha,
        &source_revision_id,
        &imported_revision_id,
        None,
        &verification,
    )?;

    Ok(())
}

struct RoundtripVerification {
    source_depth: usize,
    imported_depth: usize,
}

#[allow(clippy::too_many_arguments)]
fn print_roundtrip_result(
    args: &GitRoundtripArgs,
    git_dir: &Path,
    git_ref: &str,
    exported_commit_sha: &[u8; 20],
    source_revision_id: &ObjectId,
    imported_revision_id: &ObjectId,
    notes_imported: Option<usize>,
    verification: &RoundtripVerification,
) -> anyhow::Result<()> {
    if args.json {
        print_json(serde_json::json!({
            "schema_version": GIT_ROUNDTRIP_JSON_SCHEMA_VERSION,
            "action": "git-roundtrip",
            "verified": true,
            "source_ref": args.ref_name,
            "source_revision": source_revision_id.to_hex(),
            "git_dir": git_dir.display().to_string(),
            "exported_git_ref": git_ref,
            "exported_git_commit": hex::encode(exported_commit_sha),
            "import_ref": args.import_ref,
            "imported_revision": imported_revision_id.to_hex(),
            "with_notes": args.with_notes,
            "notes_ref": if args.with_notes { Some(args.notes_ref.as_str()) } else { None },
            "notes_imported": notes_imported,
            "checks": {
                "tree": true,
                "change_linkage": true,
                "ancestry": true,
                "source_revision_count": verification.source_depth,
                "imported_revision_count": verification.imported_depth,
            },
        }))?;
    } else {
        println!("Roundtrip verified.");
        println!("  Source ref: {}", args.ref_name);
        println!("  Source revision: {}", source_revision_id.to_hex());
        println!("  Exported git ref: {git_ref}");
        println!("  Imported ref: {}", args.import_ref);
        println!("  Imported revision: {}", imported_revision_id.to_hex());
        if let Some(notes_imported) = notes_imported {
            println!(
                "  Imported {notes_imported} provenance note(s) from refs/notes/{}",
                args.notes_ref
            );
        }
    }

    Ok(())
}

fn verify_roundtrip(
    store: &ClawStore,
    source_revision_id: &ObjectId,
    imported_revision_id: &ObjectId,
) -> anyhow::Result<RoundtripVerification> {
    let source_revision = load_revision(store, source_revision_id)?;
    let imported_revision = load_revision(store, imported_revision_id)?;

    if source_revision.tree != imported_revision.tree {
        anyhow::bail!(
            "roundtrip tree mismatch: source={:?} imported={:?}",
            source_revision.tree,
            imported_revision.tree
        );
    }

    if source_revision.change_id != imported_revision.change_id {
        anyhow::bail!(
            "roundtrip change linkage mismatch: source={:?} imported={:?}",
            source_revision.change_id,
            imported_revision.change_id
        );
    }

    let source_depth = reachable_revision_count(store, source_revision_id)?;
    let imported_depth = reachable_revision_count(store, imported_revision_id)?;
    if source_depth != imported_depth {
        anyhow::bail!(
            "roundtrip ancestry mismatch: source={} imported={}",
            source_depth,
            imported_depth
        );
    }

    Ok(RoundtripVerification {
        source_depth,
        imported_depth,
    })
}

fn print_json(value: serde_json::Value) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn load_revision(store: &ClawStore, id: &ObjectId) -> anyhow::Result<Revision> {
    let obj = store.load_object(id)?;
    match obj {
        Object::Revision(revision) => Ok(revision),
        _ => anyhow::bail!("object is not a revision: {}", id.to_hex()),
    }
}

fn reachable_revision_count(store: &ClawStore, start: &ObjectId) -> anyhow::Result<usize> {
    let mut visited = std::collections::HashSet::new();
    let mut queue = vec![*start];

    while let Some(revision_id) = queue.pop() {
        if !visited.insert(revision_id) {
            continue;
        }
        let revision = load_revision(store, &revision_id)?;
        queue.extend_from_slice(&revision.parents);
    }

    Ok(visited.len())
}

fn write_provenance_notes(
    store: &ClawStore,
    exporter: &GitExporter,
    start: &ObjectId,
    git_dir: &Path,
    notes_ref: &str,
) -> anyhow::Result<()> {
    for revision_id in collect_revision_ids(store, start)? {
        let revision = load_revision(store, &revision_id)?;
        let Some(note) = GitProvenanceNote::from_revision(store, &revision_id, &revision)? else {
            continue;
        };
        let Some(commit_sha) = exporter.get_sha1(&revision_id) else {
            continue;
        };
        write_note(git_dir, notes_ref, &hex::encode(commit_sha), &note)?;
    }
    Ok(())
}

fn collect_revision_ids(store: &ClawStore, start: &ObjectId) -> anyhow::Result<Vec<ObjectId>> {
    let mut out = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let mut queue = vec![*start];

    while let Some(id) = queue.pop() {
        if !visited.insert(id) {
            continue;
        }
        let revision = load_revision(store, &id)?;
        out.push(id);
        queue.extend_from_slice(&revision.parents);
    }

    Ok(out)
}

fn write_git_branch_ref(git_dir: &Path, git_ref: &str, sha1: &[u8; 20]) -> anyhow::Result<()> {
    let ref_path = git_dir.join(git_ref);
    write_bytes_atomic(&ref_path, format!("{}\n", hex::encode(sha1)).as_bytes())?;
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

fn normalize_git_branch_ref(branch: &str) -> String {
    if branch.starts_with("refs/heads/") {
        branch.to_string()
    } else {
        format!("refs/heads/{branch}")
    }
}

fn validate_relative_ref_path(path: &str) -> anyhow::Result<()> {
    let value = Path::new(path);
    if value.is_absolute() {
        anyhow::bail!("path must be relative: {path}");
    }

    for component in value.components() {
        match component {
            Component::Normal(_) => {}
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => anyhow::bail!("invalid path component in: {path}"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{normalize_git_branch_ref, validate_relative_ref_path, write_git_branch_ref};

    #[test]
    fn writes_ref_under_git_refs_hierarchy() {
        let tmp = tempfile::tempdir().unwrap();
        let git_dir = tmp.path().join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();

        let sha1 = [0xabu8; 20];
        write_git_branch_ref(&git_dir, "refs/heads/claw/verify", &sha1).unwrap();

        let expected = git_dir.join("refs/heads/claw/verify");
        assert!(expected.exists());
        assert_eq!(
            std::fs::read_to_string(expected).unwrap(),
            format!("{}\n", hex::encode(sha1))
        );
    }

    #[test]
    fn normalizes_git_branch_ref() {
        assert_eq!(
            normalize_git_branch_ref("claw/verify"),
            "refs/heads/claw/verify"
        );
        assert_eq!(
            normalize_git_branch_ref("refs/heads/main"),
            "refs/heads/main"
        );
    }

    #[test]
    fn validates_relative_paths() {
        assert!(validate_relative_ref_path("heads/main").is_ok());
        assert!(validate_relative_ref_path("refs/heads/main").is_ok());
        assert!(validate_relative_ref_path("../bad").is_err());
        assert!(validate_relative_ref_path("/abs").is_err());
    }
}
