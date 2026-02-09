use claw_core::id::ObjectId;

use crate::layout::RepoLayout;
use crate::refs;
use crate::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadState {
    Symbolic { ref_name: String },
    Detached { target: ObjectId },
}

pub fn read_head(layout: &RepoLayout) -> Result<HeadState, StoreError> {
    let path = layout.head_file();
    if !path.exists() {
        return Ok(HeadState::Symbolic {
            ref_name: "heads/main".to_string(),
        });
    }
    let content = std::fs::read_to_string(&path)?;
    let trimmed = content.trim();
    if let Some(ref_name) = trimmed.strip_prefix("ref: ") {
        Ok(HeadState::Symbolic {
            ref_name: ref_name.to_string(),
        })
    } else {
        let id = ObjectId::from_hex(trimmed)?;
        Ok(HeadState::Detached { target: id })
    }
}

pub fn write_head(layout: &RepoLayout, state: &HeadState) -> Result<(), StoreError> {
    let content = match state {
        HeadState::Symbolic { ref_name } => format!("ref: {}\n", ref_name),
        HeadState::Detached { target } => format!("{}\n", target.to_hex()),
    };
    std::fs::write(layout.head_file(), content)?;
    Ok(())
}

pub fn resolve_head(layout: &RepoLayout) -> Result<Option<ObjectId>, StoreError> {
    let state = read_head(layout)?;
    match state {
        HeadState::Symbolic { ref_name } => refs::read_ref(layout, &ref_name),
        HeadState::Detached { target } => Ok(Some(target)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claw_core::hash::content_hash;
    use claw_core::object::TypeTag;

    #[test]
    fn head_symbolic_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = RepoLayout::new(tmp.path());
        layout.create_dirs().unwrap();

        let state = HeadState::Symbolic {
            ref_name: "heads/main".to_string(),
        };
        write_head(&layout, &state).unwrap();
        let read_back = read_head(&layout).unwrap();
        assert_eq!(read_back, state);
    }

    #[test]
    fn head_detached_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = RepoLayout::new(tmp.path());
        layout.create_dirs().unwrap();

        let id = content_hash(TypeTag::Blob, b"test");
        let state = HeadState::Detached { target: id };
        write_head(&layout, &state).unwrap();
        let read_back = read_head(&layout).unwrap();
        assert_eq!(read_back, state);
    }

    #[test]
    fn resolve_head_symbolic() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = RepoLayout::new(tmp.path());
        layout.create_dirs().unwrap();

        let id = content_hash(TypeTag::Blob, b"test");
        refs::write_ref(&layout, "heads/main", &id).unwrap();
        write_head(
            &layout,
            &HeadState::Symbolic {
                ref_name: "heads/main".to_string(),
            },
        )
        .unwrap();

        let resolved = resolve_head(&layout).unwrap();
        assert_eq!(resolved, Some(id));
    }
}
