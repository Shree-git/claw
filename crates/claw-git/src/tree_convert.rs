use claw_core::types::{FileMode, Tree};

/// Convert a claw Tree to git tree object bytes.
/// Git tree entry format: "<mode> <name>\0<20-byte-sha1>"
pub fn to_git_tree(tree: &Tree, sha1_lookup: &dyn Fn(&claw_core::id::ObjectId) -> Option<[u8; 20]>) -> Option<Vec<u8>> {
    let mut entries_data = Vec::new();

    // Sort entries by name (git requirement)
    let mut sorted_entries = tree.entries.clone();
    sorted_entries.sort_by(|a, b| {
        let a_name = if a.mode == FileMode::Directory {
            format!("{}/", a.name)
        } else {
            a.name.clone()
        };
        let b_name = if b.mode == FileMode::Directory {
            format!("{}/", b.name)
        } else {
            b.name.clone()
        };
        a_name.cmp(&b_name)
    });

    for entry in &sorted_entries {
        let mode = match entry.mode {
            FileMode::Regular => "100644",
            FileMode::Executable => "100755",
            FileMode::Symlink => "120000",
            FileMode::Directory => "40000",
        };

        let sha1 = sha1_lookup(&entry.object_id)?;

        entries_data.extend_from_slice(mode.as_bytes());
        entries_data.push(b' ');
        entries_data.extend_from_slice(entry.name.as_bytes());
        entries_data.push(0);
        entries_data.extend_from_slice(&sha1);
    }

    let header = format!("tree {}\0", entries_data.len());
    let mut result = Vec::with_capacity(header.len() + entries_data.len());
    result.extend_from_slice(header.as_bytes());
    result.extend_from_slice(&entries_data);
    Some(result)
}
