pub mod error;
pub mod index;
pub mod layout;
pub mod lockfile;
pub mod loose;
pub mod pack;
pub mod refs;
pub mod repo;

pub use error::StoreError;

use std::path::Path;

use claw_core::cof::{cof_decode, cof_encode};
use claw_core::hash::content_hash;
use claw_core::id::ObjectId;
use claw_core::object::Object;

use crate::layout::RepoLayout;

pub struct ClawStore {
    layout: RepoLayout,
}

impl ClawStore {
    pub fn init(root: &Path) -> Result<Self, StoreError> {
        let layout = RepoLayout::new(root);
        layout.create_dirs()?;
        repo::write_default_config(&layout)?;
        Ok(Self { layout })
    }

    pub fn open(root: &Path) -> Result<Self, StoreError> {
        let layout = RepoLayout::new(root);
        if !layout.claw_dir().exists() {
            return Err(StoreError::NotARepository(root.to_path_buf()));
        }
        Ok(Self { layout })
    }

    pub fn root(&self) -> &Path {
        self.layout.root()
    }

    pub fn layout(&self) -> &RepoLayout {
        &self.layout
    }

    pub fn store_object(&self, obj: &Object) -> Result<ObjectId, StoreError> {
        let payload = obj.serialize_payload()?;
        let type_tag = obj.type_tag();
        let id = content_hash(type_tag, &payload);
        let cof_data = cof_encode(type_tag, &payload)?;
        loose::write_loose_object(&self.layout, &id, &cof_data)?;
        Ok(id)
    }

    pub fn load_object(&self, id: &ObjectId) -> Result<Object, StoreError> {
        let cof_data = loose::read_loose_object(&self.layout, id)?;
        let (type_tag, payload) = cof_decode(&cof_data)?;
        let obj = Object::deserialize_payload(type_tag, &payload)?;
        Ok(obj)
    }

    pub fn has_object(&self, id: &ObjectId) -> bool {
        loose::loose_object_path(&self.layout, id).exists()
    }

    pub fn set_ref(&self, name: &str, target: &ObjectId) -> Result<(), StoreError> {
        refs::write_ref(&self.layout, name, target)
    }

    pub fn get_ref(&self, name: &str) -> Result<Option<ObjectId>, StoreError> {
        refs::read_ref(&self.layout, name)
    }

    pub fn list_refs(&self, prefix: &str) -> Result<Vec<(String, ObjectId)>, StoreError> {
        refs::list_refs(&self.layout, prefix)
    }

    pub fn delete_ref(&self, name: &str) -> Result<(), StoreError> {
        refs::delete_ref(&self.layout, name)
    }
}
