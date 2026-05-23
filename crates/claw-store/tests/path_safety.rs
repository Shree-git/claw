use claw_core::object::TypeTag;
use claw_core::types::{validate_tree_entry_name, FileMode, Tree, TreeEntry};
use claw_core::{content_hash, ObjectId};
use claw_store::refs::{
    delete_ref, list_refs, read_ref, update_ref_cas, validate_ref_name, write_ref,
};
use claw_store::{ClawStore, StoreError};

#[test]
fn refs_reject_windows_separators_before_touching_disk() {
    let tmp = tempfile::tempdir().expect("temp repo");
    let store = ClawStore::init(tmp.path()).expect("init store");
    let id = content_hash(TypeTag::Blob, b"path safety");

    for name in [
        r"heads\main",
        r"heads/main\feature",
        r"C:\repo\.claw\refs\heads\main",
        r"\server\share\refs\main",
    ] {
        let err = validate_ref_name(name).expect_err("ref name should be rejected");
        assert!(matches!(err, StoreError::InvalidRefName(_)));

        let err = write_ref(store.layout(), name, &id).expect_err("write_ref should reject name");
        assert!(matches!(err, StoreError::InvalidRefName(_)));
    }

    assert!(
        !store.layout().refs_dir().join(r"heads\main").exists(),
        "backslash ref names must not be materialized as literal files"
    );
}

#[test]
fn refs_reject_non_portable_components_before_touching_disk() {
    let tmp = tempfile::tempdir().expect("temp repo");
    let store = ClawStore::init(tmp.path()).expect("init store");
    let id = content_hash(TypeTag::Blob, b"path safety");

    for name in [
        "heads/name:stream",
        "heads/name*glob",
        "heads/name?query",
        "heads/name<in",
        "heads/name>out",
        "heads/pipe|name",
        "heads/quoted\"name",
        "heads/trailing-dot.",
        "heads/trailing-space ",
        "heads/CON",
        "heads/con.txt",
        "heads/PRN",
        "heads/AUX",
        "heads/NUL",
        "heads/COM1",
        "heads/com9.log",
        "heads/LPT1",
        "heads/lpt9.txt",
    ] {
        let err = validate_ref_name(name).expect_err("ref name should be rejected");
        assert!(matches!(err, StoreError::InvalidRefName(_)));

        let err = write_ref(store.layout(), name, &id).expect_err("write_ref should reject name");
        assert!(matches!(err, StoreError::InvalidRefName(_)));
    }

    let overlong_component = format!("heads/{}", "a".repeat(256));
    let err =
        write_ref(store.layout(), &overlong_component, &id).expect_err("write_ref should reject");
    assert!(matches!(err, StoreError::InvalidRefName(_)));
}

#[test]
fn refs_reject_case_insensitive_collisions_before_touching_disk() {
    let tmp = tempfile::tempdir().expect("temp repo");
    let store = ClawStore::init(tmp.path()).expect("init store");
    let main = content_hash(TypeTag::Blob, b"main");
    let other = content_hash(TypeTag::Blob, b"other");

    write_ref(store.layout(), "heads/main", &main).expect("write normal ref");

    let err = write_ref(store.layout(), "heads/MAIN", &other)
        .expect_err("case-insensitive sibling ref should be rejected");
    assert!(matches!(err, StoreError::RefNameCollision { .. }));
    assert!(!std::fs::read_dir(store.layout().refs_dir().join("heads"))
        .unwrap()
        .any(|entry| entry.unwrap().file_name() == "MAIN"));

    let err = read_ref(store.layout(), "heads/MAIN")
        .expect_err("case-insensitive read should not resolve wrong ref");
    assert!(matches!(err, StoreError::RefNameCollision { .. }));

    let err = delete_ref(store.layout(), "heads/MAIN")
        .expect_err("case-insensitive delete should not remove wrong ref");
    assert!(matches!(err, StoreError::RefNameCollision { .. }));
    assert_eq!(read_ref(store.layout(), "heads/main").unwrap(), Some(main));

    let err = update_ref_cas(
        store.layout(),
        "HEADS/dev",
        None,
        &other,
        "test",
        "case collision",
    )
    .expect_err("case-insensitive parent ref should be rejected before locking");
    assert!(matches!(err, StoreError::RefNameCollision { .. }));
    assert!(!store.layout().refs_dir().join("HEADS/dev.lock").exists());
}

#[test]
fn ref_prefix_listing_rejects_windows_style_traversal_inputs() {
    let tmp = tempfile::tempdir().expect("temp repo");
    let store = ClawStore::init(tmp.path()).expect("init store");

    for prefix in [r"heads\", r"..\outside", r"C:\repo\refs"] {
        let err = list_refs(store.layout(), prefix).expect_err("prefix should be rejected");
        assert!(matches!(err, StoreError::InvalidRefName(_)));
    }

    let id = content_hash(TypeTag::Blob, b"main");
    write_ref(store.layout(), "heads/main", &id).expect("write normal ref");
    assert_eq!(list_refs(store.layout(), "heads").unwrap().len(), 1);
}

#[test]
fn tree_entries_reject_windows_path_like_names() {
    for name in [
        r"dir\file.txt",
        r"C:\Users\project\file.txt",
        "C:/Users/project/file.txt",
        r"..\outside",
        "name:stream",
        "name*glob",
        "name?query",
        "name<in",
        "name>out",
        "pipe|name",
        "quoted\"name",
        "trailing-dot.",
        "trailing-space ",
        "CON",
        "con.txt",
        "PRN",
        "AUX",
        "NUL",
        "COM1",
        "com9.log",
        "LPT1",
        "lpt9.txt",
    ] {
        assert!(
            validate_tree_entry_name(name).is_err(),
            "tree entry name should be rejected: {name}"
        );
    }

    let tree = Tree {
        entries: vec![TreeEntry {
            name: r"nested\file.txt".to_string(),
            mode: FileMode::Regular,
            object_id: ObjectId::from_bytes([0x11; 32]),
        }],
    };
    assert!(tree.validate().is_err());
}

#[test]
fn tree_entries_accept_portable_unicode_spaces_and_long_names() {
    let long_name = "a".repeat(255);
    for name in [
        "notes with spaces.txt",
        "unicodé-資料.txt",
        "crlf-fixture.txt",
        long_name.as_str(),
    ] {
        validate_tree_entry_name(name).unwrap_or_else(|err| {
            panic!("portable tree entry name should be accepted: {name}: {err}")
        });
    }
}

#[test]
fn tree_entries_reject_overlong_basenames() {
    let name = "a".repeat(256);
    let err = validate_tree_entry_name(&name).expect_err("overlong basename should be rejected");
    assert!(
        err.to_string().contains("invalid tree entry name"),
        "unexpected error: {err}"
    );
}

#[test]
fn tree_storage_rejects_case_insensitive_name_collisions() {
    let tmp = tempfile::tempdir().expect("temp repo");
    let store = ClawStore::init(tmp.path()).expect("init store");

    let err = store
        .store_object(&claw_core::object::Object::Tree(Tree {
            entries: vec![
                TreeEntry {
                    name: "README.md".to_string(),
                    mode: FileMode::Regular,
                    object_id: ObjectId::from_bytes([0x11; 32]),
                },
                TreeEntry {
                    name: "readme.md".to_string(),
                    mode: FileMode::Regular,
                    object_id: ObjectId::from_bytes([0x22; 32]),
                },
            ],
        }))
        .expect_err("case-insensitive tree entry collisions should not be stored");

    assert!(
        err.to_string().contains("case-insensitive filesystems"),
        "unexpected error: {err}"
    );
}

#[cfg(windows)]
#[test]
fn windows_absolute_ref_paths_are_rejected_by_platform_prefix_parser() {
    let err = validate_ref_name("C:/repo/.claw/refs/heads/main")
        .expect_err("windows absolute paths should not be valid refs");
    assert!(matches!(err, StoreError::InvalidRefName(_)));
}
