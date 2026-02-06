use claw_core::types::Revision;

/// Convert a claw Revision to git commit object bytes.
/// Git commit format:
/// tree <hex-sha1>
/// parent <hex-sha1>  (one per parent)
/// author <name> <email> <timestamp> <tz>
/// committer <name> <email> <timestamp> <tz>
///
/// <message>
pub fn to_git_commit(
    rev: &Revision,
    tree_sha1: &[u8; 20],
    parent_sha1s: &[[u8; 20]],
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
    // created_at_ms is milliseconds, git uses seconds
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

    let header = format!("commit {}\0", content.len());
    let mut result = Vec::with_capacity(header.len() + content.len());
    result.extend_from_slice(header.as_bytes());
    result.extend_from_slice(content.as_bytes());
    result
}
