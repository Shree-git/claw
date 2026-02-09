use claw_core::id::ObjectId;
use claw_core::types::Revision;

/// Convert a claw Revision to git commit object bytes.
pub fn to_git_commit(
    rev: &Revision,
    tree_sha1: &[u8; 20],
    parent_sha1s: &[[u8; 20]],
    rev_id: &ObjectId,
    change_id: Option<&claw_core::id::ChangeId>,
    intent_id: Option<&claw_core::id::IntentId>,
) -> Vec<u8> {
    let mut content = String::new();

    content.push_str(&format!("tree {}\n", hex::encode(tree_sha1)));

    for parent in parent_sha1s {
        content.push_str(&format!("parent {}\n", hex::encode(parent)));
    }

    let author = if rev.author.is_empty() {
        "Unknown"
    } else {
        &rev.author
    };
    let timestamp = rev.created_at_ms / 1000;

    content.push_str(&format!(
        "author {} <{}@claw> {} +0000\n",
        author, author, timestamp
    ));
    content.push_str(&format!(
        "committer {} <{}@claw> {} +0000\n",
        author, author, timestamp
    ));
    content.push('\n');
    content.push_str(&rev.summary);
    if !rev.summary.ends_with('\n') {
        content.push('\n');
    }

    // Trailers
    content.push_str(&format!("\nClaw-Revision: {}\n", rev_id.to_hex()));
    if let Some(cid) = change_id {
        content.push_str(&format!("Claw-Change: {}\n", cid));
    }
    if let Some(iid) = intent_id {
        content.push_str(&format!("Claw-Intent: {}\n", iid));
    }

    let header = format!("commit {}\0", content.len());
    let mut result = Vec::with_capacity(header.len() + content.len());
    result.extend_from_slice(header.as_bytes());
    result.extend_from_slice(content.as_bytes());
    result
}
