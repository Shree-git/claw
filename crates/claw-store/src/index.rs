use crate::StoreError;

/// Index operations backed by redb.
/// In MVP, the index is optional - we can operate purely on loose objects and ref files.
/// This provides an acceleration layer for type lookups and ref searches.

pub struct MetaIndex {
    _db: redb::Database,
}

const OBJECT_TYPE_TABLE: redb::TableDefinition<&[u8], u8> =
    redb::TableDefinition::new("object_types");

impl MetaIndex {
    pub fn open(path: &std::path::Path) -> Result<Self, StoreError> {
        let db = redb::Database::create(path)
            .map_err(|e| StoreError::Index(e.to_string()))?;
        Ok(Self { _db: db })
    }

    pub fn record_object(&self, id: &claw_core::id::ObjectId, type_tag: u8) -> Result<(), StoreError> {
        let write_txn = self._db.begin_write()
            .map_err(|e| StoreError::Index(e.to_string()))?;
        {
            let mut table = write_txn.open_table(OBJECT_TYPE_TABLE)
                .map_err(|e| StoreError::Index(e.to_string()))?;
            table.insert(id.as_bytes().as_slice(), type_tag)
                .map_err(|e| StoreError::Index(e.to_string()))?;
        }
        write_txn.commit()
            .map_err(|e| StoreError::Index(e.to_string()))?;
        Ok(())
    }

    pub fn get_type(&self, id: &claw_core::id::ObjectId) -> Result<Option<u8>, StoreError> {
        let read_txn = self._db.begin_read()
            .map_err(|e| StoreError::Index(e.to_string()))?;
        let table = read_txn.open_table(OBJECT_TYPE_TABLE)
            .map_err(|e| StoreError::Index(e.to_string()))?;
        let result = table.get(id.as_bytes().as_slice())
            .map_err(|e| StoreError::Index(e.to_string()))?;
        Ok(result.map(|v| v.value()))
    }
}
