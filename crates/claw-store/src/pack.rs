use claw_core::cof::{cof_decode, cof_encode};
use claw_core::hash::content_hash;
use claw_core::id::ObjectId;
use claw_core::object::Object;

use crate::layout::RepoLayout;
use crate::StoreError;

/// Pack format (MVP - no delta compression):
/// [4B "CLPK"][4B version=1][4B object_count]
/// [object entries: 4B length, COF bytes]*
const PACK_MAGIC: &[u8; 4] = b"CLPK";
const PACK_VERSION: u32 = 1;

/// Index format (separate .idx file):
/// [4B "CLIX"][4B entry_count]
/// [entries: 32B ObjectId, 8B offset]*
const IDX_MAGIC: &[u8; 4] = b"CLIX";

pub struct PackWriter {
    objects: Vec<(ObjectId, Vec<u8>)>,
}

impl Default for PackWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl PackWriter {
    pub fn new() -> Self {
        Self {
            objects: Vec::new(),
        }
    }

    pub fn add_object(&mut self, obj: &Object) -> Result<ObjectId, StoreError> {
        let payload = obj.serialize_payload()?;
        let type_tag = obj.type_tag();
        let id = content_hash(type_tag, &payload);
        let cof_data = cof_encode(type_tag, &payload)?;
        self.objects.push((id, cof_data));
        Ok(id)
    }

    /// Write pack and index as separate files with hash-based naming.
    /// Returns (pack_path, idx_path).
    pub fn write_pack(&self, layout: &RepoLayout) -> Result<(std::path::PathBuf, std::path::PathBuf), StoreError> {
        // Build pack data
        let mut data = Vec::new();

        // Header
        data.extend_from_slice(PACK_MAGIC);
        data.extend_from_slice(&PACK_VERSION.to_le_bytes());
        data.extend_from_slice(&(self.objects.len() as u32).to_le_bytes());

        // Object entries
        let mut index_entries = Vec::new();
        for (id, cof_data) in &self.objects {
            let offset = data.len() as u64;
            let len = cof_data.len() as u32;
            data.extend_from_slice(&len.to_le_bytes());
            data.extend_from_slice(cof_data);
            index_entries.push((*id, offset));
        }

        // Hash the pack data for naming
        let pack_hash = blake3::hash(&data);
        let hash_hex = hex::encode(&pack_hash.as_bytes()[..16]); // 16 bytes = 32 hex chars

        let pack_path = layout.packs_dir().join(format!("{hash_hex}.clwpack"));
        let idx_path = layout.packs_dir().join(format!("{hash_hex}.idx"));

        // Write pack file
        std::fs::write(&pack_path, &data)?;

        // Write separate index file
        let mut idx_data = Vec::new();
        idx_data.extend_from_slice(IDX_MAGIC);
        idx_data.extend_from_slice(&(index_entries.len() as u32).to_le_bytes());
        for (id, offset) in &index_entries {
            idx_data.extend_from_slice(id.as_bytes());
            idx_data.extend_from_slice(&offset.to_le_bytes());
        }
        std::fs::write(&idx_path, &idx_data)?;

        Ok((pack_path, idx_path))
    }

    /// Legacy write with explicit name (for backward compat).
    pub fn write_pack_named(&self, layout: &RepoLayout, pack_name: &str) -> Result<std::path::PathBuf, StoreError> {
        let pack_path = layout.packs_dir().join(format!("{pack_name}.clwpack"));

        let mut data = Vec::new();

        // Header
        data.extend_from_slice(PACK_MAGIC);
        data.extend_from_slice(&PACK_VERSION.to_le_bytes());
        data.extend_from_slice(&(self.objects.len() as u32).to_le_bytes());

        // Object entries
        let mut index_entries = Vec::new();
        for (id, cof_data) in &self.objects {
            let offset = data.len() as u64;
            let len = cof_data.len() as u32;
            data.extend_from_slice(&len.to_le_bytes());
            data.extend_from_slice(cof_data);
            index_entries.push((*id, offset));
        }

        std::fs::write(&pack_path, &data)?;

        // Write separate .idx
        let idx_path = layout.packs_dir().join(format!("{pack_name}.idx"));
        let mut idx_data = Vec::new();
        idx_data.extend_from_slice(IDX_MAGIC);
        idx_data.extend_from_slice(&(index_entries.len() as u32).to_le_bytes());
        for (id, offset) in &index_entries {
            idx_data.extend_from_slice(id.as_bytes());
            idx_data.extend_from_slice(&offset.to_le_bytes());
        }
        std::fs::write(&idx_path, &idx_data)?;

        Ok(pack_path)
    }
}

pub fn read_pack_index(idx_path: &std::path::Path) -> Result<Vec<(ObjectId, u64)>, StoreError> {
    let data = std::fs::read(idx_path)?;

    // Try new separate index format first
    if data.len() >= 8 && &data[..4] == IDX_MAGIC {
        let entry_count = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
        let mut entries = Vec::with_capacity(entry_count);
        let mut pos = 8;
        for _ in 0..entry_count {
            if pos + 40 > data.len() {
                break;
            }
            let mut id_bytes = [0u8; 32];
            id_bytes.copy_from_slice(&data[pos..pos + 32]);
            pos += 32;
            let offset = u64::from_le_bytes([
                data[pos], data[pos + 1], data[pos + 2], data[pos + 3],
                data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7],
            ]);
            pos += 8;
            entries.push((ObjectId::from_bytes(id_bytes), offset));
        }
        return Ok(entries);
    }

    // Fall back to reading inline index from pack file
    read_pack_index_inline(idx_path)
}

/// Read inline index from legacy pack files that embed the index.
fn read_pack_index_inline(pack_path: &std::path::Path) -> Result<Vec<(ObjectId, u64)>, StoreError> {
    let data = std::fs::read(pack_path)?;
    if data.len() < 16 || &data[..4] != PACK_MAGIC {
        return Err(StoreError::Config("invalid pack file".into()));
    }

    // Read index count from end
    let idx_count_offset = data.len() - 4;
    let idx_count = u32::from_le_bytes([
        data[idx_count_offset],
        data[idx_count_offset + 1],
        data[idx_count_offset + 2],
        data[idx_count_offset + 3],
    ]) as usize;

    let idx_start = idx_count_offset - (idx_count * 40); // 32 bytes id + 8 bytes offset
    let mut entries = Vec::with_capacity(idx_count);
    let mut pos = idx_start;
    for _ in 0..idx_count {
        let mut id_bytes = [0u8; 32];
        id_bytes.copy_from_slice(&data[pos..pos + 32]);
        pos += 32;
        let offset = u64::from_le_bytes([
            data[pos], data[pos + 1], data[pos + 2], data[pos + 3],
            data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7],
        ]);
        pos += 8;
        entries.push((ObjectId::from_bytes(id_bytes), offset));
    }

    Ok(entries)
}

pub fn read_object_from_pack(pack_path: &std::path::Path, offset: u64) -> Result<Object, StoreError> {
    let data = std::fs::read(pack_path)?;
    let offset = offset as usize;
    let len = u32::from_le_bytes([
        data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
    ]) as usize;
    let cof_data = &data[offset + 4..offset + 4 + len];
    let (type_tag, payload) = cof_decode(cof_data)?;
    let obj = Object::deserialize_payload(type_tag, &payload)?;
    Ok(obj)
}
