use std::collections::HashMap;
use std::path::Path;

use claw_core::id::ObjectId;
use claw_core::object::Object;
use claw_store::ClawStore;

use crate::blob_convert::{git_sha1, to_git_blob};
use crate::commit_convert::to_git_commit;
use crate::tree_convert::to_git_tree;
use crate::GitExportError;

pub struct GitExporter<'a> {
    store: &'a ClawStore,
    /// Maps claw ObjectId -> git SHA-1
    sha1_map: HashMap<ObjectId, [u8; 20]>,
}

impl<'a> GitExporter<'a> {
    pub fn new(store: &'a ClawStore) -> Self {
        Self {
            store,
            sha1_map: HashMap::new(),
        }
    }

    pub fn get_sha1(&self, claw_id: &ObjectId) -> Option<[u8; 20]> {
        self.sha1_map.get(claw_id).copied()
    }

    /// Export the claw DAG starting from a revision to a git object directory.
    pub fn export(&mut self, head: &ObjectId, git_objects_dir: &Path) -> Result<[u8; 20], GitExportError> {
        std::fs::create_dir_all(git_objects_dir)?;
        self.export_revision(head, git_objects_dir)
    }

    fn export_revision(&mut self, rev_id: &ObjectId, git_dir: &Path) -> Result<[u8; 20], GitExportError> {
        if let Some(sha1) = self.sha1_map.get(rev_id) {
            return Ok(*sha1);
        }

        let obj = self.store.load_object(rev_id)?;
        let rev = match obj {
            Object::Revision(r) => r,
            _ => return Err(GitExportError::InvalidType("expected revision".into())),
        };

        // Export parents first
        let mut parent_sha1s = Vec::new();
        for parent in &rev.parents {
            let sha1 = self.export_revision(parent, git_dir)?;
            parent_sha1s.push(sha1);
        }

        // Export tree
        let tree_id = rev.tree.ok_or_else(|| GitExportError::InvalidType("revision has no tree".into()))?;
        let tree_sha1 = self.export_tree(&tree_id, git_dir)?;

        // Resolve intent_id from change if available
        let intent_id = rev.change_id.as_ref().and_then(|cid| {
            let change_ref = format!("changes/{}", cid);
            let change_obj_id = self.store.get_ref(&change_ref).ok()??;
            let change_obj = self.store.load_object(&change_obj_id).ok()?;
            if let Object::Change(c) = change_obj {
                Some(c.intent_id)
            } else {
                None
            }
        });

        // Build commit
        let commit_data = to_git_commit(&rev, &tree_sha1, &parent_sha1s, rev_id, rev.change_id.as_ref(), intent_id.as_ref());
        let sha1 = git_sha1(&commit_data);
        self.write_git_object(git_dir, &sha1, &commit_data)?;
        self.sha1_map.insert(*rev_id, sha1);

        Ok(sha1)
    }

    fn export_tree(&mut self, tree_id: &ObjectId, git_dir: &Path) -> Result<[u8; 20], GitExportError> {
        if let Some(sha1) = self.sha1_map.get(tree_id) {
            return Ok(*sha1);
        }

        let obj = self.store.load_object(tree_id)?;
        let tree = match obj {
            Object::Tree(t) => t,
            _ => return Err(GitExportError::InvalidType("expected tree".into())),
        };

        // Export all entries first
        for entry in &tree.entries {
            match entry.mode {
                claw_core::types::FileMode::Directory => {
                    self.export_tree(&entry.object_id, git_dir)?;
                }
                _ => {
                    self.export_blob(&entry.object_id, git_dir)?;
                }
            };
        }

        let sha1_map = &self.sha1_map;
        let tree_data = to_git_tree(&tree, &|id| sha1_map.get(id).copied())
            .ok_or_else(|| GitExportError::ObjectNotFound("tree entry sha1 not found".into()))?;
        let sha1 = git_sha1(&tree_data);
        self.write_git_object(git_dir, &sha1, &tree_data)?;
        self.sha1_map.insert(*tree_id, sha1);

        Ok(sha1)
    }

    fn export_blob(&mut self, blob_id: &ObjectId, git_dir: &Path) -> Result<[u8; 20], GitExportError> {
        if let Some(sha1) = self.sha1_map.get(blob_id) {
            return Ok(*sha1);
        }

        let obj = self.store.load_object(blob_id)?;
        let blob = match obj {
            Object::Blob(b) => b,
            _ => return Err(GitExportError::InvalidType("expected blob".into())),
        };

        let git_data = to_git_blob(&blob.data);
        let sha1 = git_sha1(&git_data);
        self.write_git_object(git_dir, &sha1, &git_data)?;
        self.sha1_map.insert(*blob_id, sha1);

        Ok(sha1)
    }

    fn write_git_object(&self, git_dir: &Path, sha1: &[u8; 20], data: &[u8]) -> Result<(), GitExportError> {
        let hex = hex::encode(sha1);
        let dir = git_dir.join(&hex[..2]);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(&hex[2..]);
        if !path.exists() {
            // Git stores objects zlib-compressed
            let compressed = miniz_compress(data);
            std::fs::write(&path, &compressed)?;
        }
        Ok(())
    }
}

/// Minimal zlib/deflate compression (for git object storage)
fn miniz_compress(data: &[u8]) -> Vec<u8> {
    // Use zstd as a stand-in -- but git uses zlib.
    // For proper git compat we need zlib. Let's implement a store-only mode
    // that writes raw git objects (uncompressed) -- git can handle this with
    // `git unpack-objects` or we add proper zlib later.
    //
    // For MVP determinism testing, write a simple zlib wrapper:
    // zlib header (0x78, 0x01 = no compression) + raw data + adler32
    let mut result = Vec::with_capacity(data.len() + 11);
    // zlib header for no compression
    result.push(0x78);
    result.push(0x01);

    // Deflate "stored" blocks
    let mut offset = 0;
    while offset < data.len() {
        let remaining = data.len() - offset;
        let block_size = remaining.min(65535);
        let is_final = offset + block_size >= data.len();

        result.push(if is_final { 0x01 } else { 0x00 });
        let len = block_size as u16;
        result.extend_from_slice(&len.to_le_bytes());
        let nlen = !len;
        result.extend_from_slice(&nlen.to_le_bytes());
        result.extend_from_slice(&data[offset..offset + block_size]);
        offset += block_size;
    }

    // Adler-32 checksum
    let adler = adler32(data);
    result.extend_from_slice(&adler.to_be_bytes());

    result
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}
