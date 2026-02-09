use claw_core::cof::cof_encode;
use claw_core::hash::content_hash;
use claw_core::id::{ChangeId, IntentId};
use claw_core::object::{Object, TypeTag};
use claw_core::types::*;
use claw_store::ClawStore;

fn make_test_store() -> (tempfile::TempDir, ClawStore) {
    let tmp = tempfile::tempdir().unwrap();
    let store = ClawStore::init(tmp.path()).unwrap();
    (tmp, store)
}

// === Test 1: Object round-trip for all 12 types ===
#[test]
fn test_object_roundtrip_all_12_types() {
    let (_tmp, store) = make_test_store();

    // 1. Blob
    let blob = Object::Blob(Blob { data: b"hello world".to_vec(), media_type: None });
    let blob_id = store.store_object(&blob).unwrap();
    let loaded = store.load_object(&blob_id).unwrap();
    if let Object::Blob(b) = loaded {
        assert_eq!(b.data, b"hello world");
    } else {
        panic!("expected Blob");
    }

    // 2. Tree
    let tree = Object::Tree(Tree {
        entries: vec![TreeEntry {
            name: "file.txt".to_string(),
            mode: FileMode::Regular,
            object_id: blob_id,
        }],
    });
    let tree_id = store.store_object(&tree).unwrap();
    let loaded = store.load_object(&tree_id).unwrap();
    assert!(matches!(loaded, Object::Tree(_)));

    // 3. Patch
    let patch = Object::Patch(Patch {
        target_path: "file.txt".to_string(),
        codec_id: "text/line".to_string(),
        base_object: Some(blob_id),
        result_object: None,
        ops: vec![PatchOp {
            address: "L0".to_string(),
            op_type: "insert".to_string(),
            old_data: None,
            new_data: Some(b"new line\n".to_vec()),
            context_hash: None,
        }],
        codec_payload: None,
    });
    let patch_id = store.store_object(&patch).unwrap();
    assert!(matches!(store.load_object(&patch_id).unwrap(), Object::Patch(_)));

    // 4. Revision
    let revision = Object::Revision(Revision {
        parents: vec![],
        tree: Some(tree_id),
        patches: vec![patch_id],
        change_id: None,
        snapshot_base: None,
        capsule_id: None,
        summary: "initial".to_string(),
        author: "test".to_string(),
        created_at_ms: 1000,
        policy_evidence: vec![],
    });
    let rev_id = store.store_object(&revision).unwrap();
    assert!(matches!(store.load_object(&rev_id).unwrap(), Object::Revision(_)));

    // 5. Snapshot
    let snapshot = Object::Snapshot(Snapshot {
        revision_id: rev_id,
        tree_root: tree_id,
        created_at_ms: 1000,
    });
    let snap_id = store.store_object(&snapshot).unwrap();
    assert!(matches!(store.load_object(&snap_id).unwrap(), Object::Snapshot(_)));

    // 6. Intent
    let intent = Object::Intent(Intent {
        id: IntentId::new(),
        title: "Test intent".to_string(),
        goal: "A test".to_string(),
        constraints: vec![],
        acceptance_tests: vec![],
        links: vec![],
        policy_refs: vec![],
        agents: vec![],
        change_ids: vec![],
        depends_on: vec![],
        supersedes: vec![],
        status: IntentStatus::Open,
        created_at_ms: 1000,
        updated_at_ms: 1000,
    });
    let intent_id = store.store_object(&intent).unwrap();
    assert!(matches!(store.load_object(&intent_id).unwrap(), Object::Intent(_)));

    // 7. Change
    let change = Object::Change(Change {
        id: ChangeId::new(),
        intent_id: IntentId::new(),
        head_revision: None,
        workstream_id: None,
        status: ChangeStatus::Open,
        created_at_ms: 1000,
        updated_at_ms: 1000,
    });
    let change_obj_id = store.store_object(&change).unwrap();
    assert!(matches!(store.load_object(&change_obj_id).unwrap(), Object::Change(_)));

    // 8. Conflict
    let conflict = Object::Conflict(Conflict {
        base_revision: Some(blob_id),
        left_revision: rev_id,
        right_revision: rev_id,
        file_path: "file.txt".to_string(),
        codec_id: "text/line".to_string(),
        left_patch_ids: vec![patch_id],
        right_patch_ids: vec![],
        resolution_patch_ids: vec![],
        status: ConflictStatus::Open,
        created_at_ms: 1000,
    });
    let conflict_obj_id = store.store_object(&conflict).unwrap();
    assert!(matches!(store.load_object(&conflict_obj_id).unwrap(), Object::Conflict(_)));

    // 9. Capsule
    let capsule = Object::Capsule(Capsule {
        revision_id: rev_id,
        public_fields: CapsulePublic {
            agent_id: "test-agent".to_string(),
            agent_version: None,
            toolchain_digest: None,
            env_fingerprint: None,
            evidence: vec![],
        },
        encrypted_private: None,
        encryption: String::new(),
        key_id: None,
        signatures: vec![],
    });
    let capsule_obj_id = store.store_object(&capsule).unwrap();
    assert!(matches!(store.load_object(&capsule_obj_id).unwrap(), Object::Capsule(_)));

    // 10. Policy
    let policy = Object::Policy(Policy {
        policy_id: "default".to_string(),
        visibility: Visibility::Public,
        required_checks: vec!["ci".to_string()],
        required_reviewers: vec![],
        sensitive_paths: vec![],
        quarantine_lane: false,
        min_trust_score: None,
    });
    let policy_obj_id = store.store_object(&policy).unwrap();
    assert!(matches!(store.load_object(&policy_obj_id).unwrap(), Object::Policy(_)));

    // 11. Workstream
    let workstream = Object::Workstream(Workstream {
        workstream_id: "main".to_string(),
        change_stack: vec![],
    });
    let ws_obj_id = store.store_object(&workstream).unwrap();
    assert!(matches!(store.load_object(&ws_obj_id).unwrap(), Object::Workstream(_)));

    // 12. RefLog
    let reflog = Object::RefLog(RefLog {
        ref_name: "heads/main".to_string(),
        entries: vec![RefLogEntry {
            old_target: None,
            new_target: rev_id,
            author: "test".to_string(),
            message: "init".to_string(),
            timestamp: 1000,
        }],
    });
    let reflog_obj_id = store.store_object(&reflog).unwrap();
    assert!(matches!(store.load_object(&reflog_obj_id).unwrap(), Object::RefLog(_)));
}

// === Test 2: Deterministic hashing across invocations ===
#[test]
fn test_deterministic_hashing() {
    let payload = b"deterministic test payload";

    let h1 = content_hash(TypeTag::Blob, payload);
    let h2 = content_hash(TypeTag::Blob, payload);
    assert_eq!(h1, h2, "hashing must be deterministic");

    // Also test that COF encoding is deterministic
    let cof1 = cof_encode(TypeTag::Blob, payload).unwrap();
    let cof2 = cof_encode(TypeTag::Blob, payload).unwrap();
    assert_eq!(cof1, cof2, "COF encoding must be deterministic");

    // Store and retrieve - same ID
    let (_tmp, store) = make_test_store();
    let obj = Object::Blob(Blob { data: payload.to_vec(), media_type: None });
    let id1 = store.store_object(&obj).unwrap();
    let id2 = store.store_object(&obj).unwrap();
    assert_eq!(id1, id2, "storing same object must produce same ID");
}

// === Test 3: Patch commute and conflict emission ===
#[test]
fn test_patch_commute_and_conflict() {
    use claw_patch::text_line::TextLineCodec;
    use claw_patch::Codec;

    let codec = TextLineCodec;

    // Non-overlapping patches should commute
    let base = b"line1\nline2\nline3\nline4\nline5\n";
    let left_new = b"line1\nleft_change\nline3\nline4\nline5\n";
    let right_new = b"line1\nline2\nline3\nline4\nright_change\n";

    let left_ops = codec.diff(base, left_new).unwrap();
    let right_ops = codec.diff(base, right_new).unwrap();

    // Non-overlapping should commute
    let commute_result = codec.commute(&left_ops, &right_ops);
    assert!(commute_result.is_ok(), "non-overlapping patches should commute");

    // Overlapping patches should fail to commute
    let left_conflict = b"line1\nleft\nline3\nline4\nline5\n";
    let right_conflict = b"line1\nright\nline3\nline4\nline5\n";

    let left_conflict_ops = codec.diff(base, left_conflict).unwrap();
    let right_conflict_ops = codec.diff(base, right_conflict).unwrap();

    let conflict_result = codec.commute(&left_conflict_ops, &right_conflict_ops);
    assert!(conflict_result.is_err(), "overlapping patches should fail to commute");
}

// === Test 4: JSON semantic merge success ===
#[test]
fn test_json_semantic_merge() {
    use claw_patch::json_tree::JsonTreeCodec;
    use claw_patch::Codec;
    use serde_json::json;

    let codec = JsonTreeCodec;

    let base = serde_json::to_vec(&json!({
        "name": "project",
        "version": "1.0",
        "deps": {"a": "1.0", "b": "2.0"}
    })).unwrap();

    let left = serde_json::to_vec(&json!({
        "name": "project",
        "version": "1.1",
        "deps": {"a": "1.0", "b": "2.0"}
    })).unwrap();

    let right = serde_json::to_vec(&json!({
        "name": "project",
        "version": "1.0",
        "deps": {"a": "1.0", "b": "2.0", "c": "3.0"}
    })).unwrap();

    let merged = codec.merge3(&base, &left, &right).unwrap();
    let merged_val: serde_json::Value = serde_json::from_slice(&merged).unwrap();

    assert_eq!(merged_val["version"], "1.1", "left change should be preserved");
    assert_eq!(merged_val["deps"]["c"], "3.0", "right addition should be preserved");
}

// === Test 5: Conflict persistence and later resolution ===
#[test]
fn test_conflict_persistence_and_resolution() {
    let (_tmp, store) = make_test_store();

    // Create base objects
    let blob = Object::Blob(Blob { data: b"base".to_vec(), media_type: None });
    let blob_id = store.store_object(&blob).unwrap();

    let tree = Object::Tree(Tree {
        entries: vec![TreeEntry {
            name: "file.txt".to_string(),
            mode: FileMode::Regular,
            object_id: blob_id,
        }],
    });
    let _tree_id = store.store_object(&tree).unwrap();

    // Create a conflict
    let rev_id = content_hash(TypeTag::Revision, b"dummy");
    let conflict = Object::Conflict(Conflict {
        base_revision: Some(blob_id),
        left_revision: rev_id,
        right_revision: rev_id,
        file_path: "file.txt".to_string(),
        codec_id: "text/line".to_string(),
        left_patch_ids: vec![],
        right_patch_ids: vec![],
        resolution_patch_ids: vec![],
        status: ConflictStatus::Open,
        created_at_ms: 1000,
    });
    let conflict_obj_id = store.store_object(&conflict).unwrap();

    // Verify conflict persists
    let loaded = store.load_object(&conflict_obj_id).unwrap();
    let loaded_conflict = match loaded {
        Object::Conflict(c) => c,
        _ => panic!("expected conflict"),
    };
    assert_eq!(loaded_conflict.status, ConflictStatus::Open);
    assert!(loaded_conflict.resolution_patch_ids.is_empty());

    // Resolve the conflict
    let resolution_blob = Object::Blob(Blob { data: b"resolved content".to_vec(), media_type: None });
    let resolution_id = store.store_object(&resolution_blob).unwrap();

    let resolved = Object::Conflict(Conflict {
        resolution_patch_ids: vec![resolution_id],
        status: ConflictStatus::Resolved,
        ..loaded_conflict
    });
    let resolved_obj_id = store.store_object(&resolved).unwrap();

    // Verify resolution persists
    let loaded_resolved = store.load_object(&resolved_obj_id).unwrap();
    if let Object::Conflict(c) = loaded_resolved {
        assert_eq!(c.status, ConflictStatus::Resolved);
        assert_eq!(c.resolution_patch_ids.len(), 1);
        assert_eq!(c.resolution_patch_ids[0], resolution_id);
    } else {
        panic!("expected conflict");
    }
}

// === Test 6: Capsule redaction enforcement ===
#[test]
fn test_capsule_redaction_enforcement() {
    use claw_crypto::encrypt::decrypt;
    use claw_crypto::keypair::KeyPair;
    use claw_crypto::capsule::build_capsule;

    let kp = KeyPair::generate();
    let enc_key = [42u8; 32];
    let wrong_key = [99u8; 32];

    let rev_id = content_hash(TypeTag::Revision, b"test");
    let private_data = b"sensitive information";

    let public = CapsulePublic {
        agent_id: "redaction-test-agent".to_string(),
        agent_version: None,
        toolchain_digest: None,
        env_fingerprint: None,
        evidence: vec![],
    };

    let capsule = build_capsule(&rev_id, public, Some(private_data), Some(&enc_key), &kp).unwrap();

    // Authorized key can decrypt
    let decrypted = decrypt(&enc_key, capsule.encrypted_private.as_ref().unwrap()).unwrap();
    assert_eq!(decrypted, private_data);

    // Wrong key cannot decrypt
    assert!(decrypt(&wrong_key, capsule.encrypted_private.as_ref().unwrap()).is_err());
}

// === Test 7: Signature verification ===
#[test]
fn test_signature_verification() {
    use claw_crypto::keypair::KeyPair;
    use claw_crypto::capsule::{build_capsule, verify_capsule};

    let kp = KeyPair::generate();
    let kp2 = KeyPair::generate();
    let rev_id = content_hash(TypeTag::Revision, b"sig test");

    let public = CapsulePublic {
        agent_id: "sig-test-agent".to_string(),
        agent_version: None,
        toolchain_digest: None,
        env_fingerprint: None,
        evidence: vec![],
    };

    let capsule = build_capsule(&rev_id, public, None, None, &kp).unwrap();

    // Correct key verifies
    let pk = kp.public_key_bytes();
    assert!(verify_capsule(&capsule, &pk).unwrap());

    // Wrong key fails
    let wrong_pk = kp2.public_key_bytes();
    assert!(!verify_capsule(&capsule, &wrong_pk).unwrap());

    // Tampered content fails
    let mut tampered = capsule.clone();
    tampered.public_fields.agent_id = "TAMPERED".to_string();
    assert!(!verify_capsule(&tampered, &pk).unwrap());
}

// === Test 8: Partial clone filter correctness ===
#[test]
fn test_partial_clone_filter() {
    use claw_sync::partial_clone::PartialCloneFilter;

    let (_tmp, store) = make_test_store();

    // Create objects
    let blob = Object::Blob(Blob { data: b"test".to_vec(), media_type: None });
    let blob_id = store.store_object(&blob).unwrap();

    let patch_rs = Object::Patch(Patch {
        target_path: "src/main.rs".to_string(),
        codec_id: "text/line".to_string(),
        base_object: Some(blob_id),
        result_object: None,
        ops: vec![],
        codec_payload: None,
    });
    let patch_rs_id = store.store_object(&patch_rs).unwrap();

    let patch_json = Object::Patch(Patch {
        target_path: "config/settings.json".to_string(),
        codec_id: "json/tree".to_string(),
        base_object: Some(blob_id),
        result_object: None,
        ops: vec![],
        codec_payload: None,
    });
    let patch_json_id = store.store_object(&patch_json).unwrap();

    // Filter by path prefix
    let path_filter = PartialCloneFilter {
        path_prefixes: vec!["src/".to_string()],
        codec_ids: vec![],
        time_range: None,
        max_depth: None,
        max_bytes: None,
    };
    assert!(path_filter.matches_object(&store, &patch_rs_id));
    assert!(!path_filter.matches_object(&store, &patch_json_id));

    // Filter by codec
    let codec_filter = PartialCloneFilter {
        path_prefixes: vec![],
        codec_ids: vec!["json/tree".to_string()],
        time_range: None,
        max_depth: None,
        max_bytes: None,
    };
    assert!(!codec_filter.matches_object(&store, &patch_rs_id));
    assert!(codec_filter.matches_object(&store, &patch_json_id));

    // Blobs always pass
    assert!(path_filter.matches_object(&store, &blob_id));
}

// === Test 9: Git export determinism ===
#[test]
fn test_git_export_determinism() {
    use claw_git::exporter::GitExporter;

    let (_tmp, store) = make_test_store();

    // Create a simple repo structure
    let blob = Object::Blob(Blob { data: b"hello world\n".to_vec(), media_type: None });
    let blob_id = store.store_object(&blob).unwrap();

    let tree = Object::Tree(Tree {
        entries: vec![TreeEntry {
            name: "hello.txt".to_string(),
            mode: FileMode::Regular,
            object_id: blob_id,
        }],
    });
    let tree_id = store.store_object(&tree).unwrap();

    let rev = Object::Revision(Revision {
        parents: vec![],
        tree: Some(tree_id),
        patches: vec![],
        change_id: None,
        snapshot_base: None,
        capsule_id: None,
        summary: "initial commit".to_string(),
        author: "test".to_string(),
        created_at_ms: 1000000,
        policy_evidence: vec![],
    });
    let rev_id = store.store_object(&rev).unwrap();

    // Export twice to different directories
    let git_dir1 = _tmp.path().join("git1");
    let git_dir2 = _tmp.path().join("git2");

    let mut exporter1 = GitExporter::new(&store);
    let sha1_1 = exporter1.export(&rev_id, &git_dir1).unwrap();

    let mut exporter2 = GitExporter::new(&store);
    let sha1_2 = exporter2.export(&rev_id, &git_dir2).unwrap();

    // Both exports should produce identical SHA-1 hashes
    assert_eq!(sha1_1, sha1_2, "git export must be deterministic");
    assert_ne!(sha1_1, [0u8; 20], "SHA-1 should not be all zeros");
}

// === Test 10: End-to-end workflow ===
// init -> create files -> snapshot -> branch -> checkout -> modify -> snapshot ->
// checkout main -> integrate -> snapshot merge -> verify final tree
#[test]
fn test_end_to_end_workflow() {
    use claw_merge::emit::merge;
    use claw_patch::CodecRegistry;
    use claw_store::HeadState;
    use claw_store::tree_diff::{diff_trees, ChangeKind};
    use std::collections::HashSet;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let store = ClawStore::init(root).unwrap();
    let registry = CodecRegistry::default();

    // === Phase 1: Initial snapshot on main ===
    // Create files in the working directory
    std::fs::write(root.join("file.txt"), "hello\n").unwrap();
    std::fs::write(root.join("data.json"), r#"{"key": "value", "count": 1}"#).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();

    // Build initial tree: store blobs + trees manually (simulating scan_worktree)
    let file_blob = Object::Blob(Blob { data: b"hello\n".to_vec(), media_type: None });
    let file_blob_id = store.store_object(&file_blob).unwrap();

    let json_blob = Object::Blob(Blob {
        data: br#"{"key": "value", "count": 1}"#.to_vec(),
        media_type: None,
    });
    let json_blob_id = store.store_object(&json_blob).unwrap();

    let main_rs_blob = Object::Blob(Blob { data: b"fn main() {}\n".to_vec(), media_type: None });
    let main_rs_blob_id = store.store_object(&main_rs_blob).unwrap();

    let src_tree = Object::Tree(Tree {
        entries: vec![TreeEntry {
            name: "main.rs".to_string(),
            mode: FileMode::Regular,
            object_id: main_rs_blob_id,
        }],
    });
    let src_tree_id = store.store_object(&src_tree).unwrap();

    let root_tree = Object::Tree(Tree {
        entries: vec![
            TreeEntry {
                name: "data.json".to_string(),
                mode: FileMode::Regular,
                object_id: json_blob_id,
            },
            TreeEntry {
                name: "file.txt".to_string(),
                mode: FileMode::Regular,
                object_id: file_blob_id,
            },
            TreeEntry {
                name: "src".to_string(),
                mode: FileMode::Directory,
                object_id: src_tree_id,
            },
        ],
    });
    let root_tree_id = store.store_object(&root_tree).unwrap();

    // Create initial revision
    let rev1 = Object::Revision(Revision {
        change_id: None,
        parents: vec![],
        patches: vec![],
        snapshot_base: None,
        tree: Some(root_tree_id),
        capsule_id: None,
        author: "test".to_string(),
        created_at_ms: 1000,
        summary: "initial commit".to_string(),
        policy_evidence: vec![],
    });
    let rev1_id = store.store_object(&rev1).unwrap();
    store.update_ref_cas("heads/main", None, &rev1_id, "test", "initial").unwrap();

    // Verify HEAD is on main
    let head = store.read_head().unwrap();
    assert_eq!(
        head,
        HeadState::Symbolic { ref_name: "heads/main".to_string() }
    );

    // Verify ref resolves
    let resolved = store.resolve_head().unwrap();
    assert_eq!(resolved, Some(rev1_id));

    // === Phase 2: Create branch "feature" ===
    store.set_ref("heads/feature", &rev1_id).unwrap();
    let branches = store.list_refs("heads/").unwrap();
    let branch_names: HashSet<String> = branches.iter().map(|(n, _)| n.clone()).collect();
    assert!(branch_names.contains("heads/main"));
    assert!(branch_names.contains("heads/feature"));

    // === Phase 3: "Checkout" feature (update HEAD) ===
    store.write_head(&HeadState::Symbolic {
        ref_name: "heads/feature".to_string(),
    }).unwrap();
    let head = store.read_head().unwrap();
    assert_eq!(
        head,
        HeadState::Symbolic { ref_name: "heads/feature".to_string() }
    );

    // === Phase 4: Modify file.txt on feature branch ===
    let modified_blob = Object::Blob(Blob { data: b"modified on feature\n".to_vec(), media_type: None });
    let modified_blob_id = store.store_object(&modified_blob).unwrap();

    let feature_tree = Object::Tree(Tree {
        entries: vec![
            TreeEntry {
                name: "data.json".to_string(),
                mode: FileMode::Regular,
                object_id: json_blob_id,
            },
            TreeEntry {
                name: "file.txt".to_string(),
                mode: FileMode::Regular,
                object_id: modified_blob_id,
            },
            TreeEntry {
                name: "src".to_string(),
                mode: FileMode::Directory,
                object_id: src_tree_id,
            },
        ],
    });
    let feature_tree_id = store.store_object(&feature_tree).unwrap();

    // Verify diff between main and feature trees
    let changes = diff_trees(&store, Some(&root_tree_id), Some(&feature_tree_id), "").unwrap();
    assert_eq!(changes.len(), 1, "should have exactly 1 changed file");
    assert_eq!(changes[0].path, "file.txt");
    assert_eq!(changes[0].kind, ChangeKind::Modified);

    // Create feature revision with patch
    let text_codec = registry.get_by_extension("txt");
    let mut patches = vec![];
    if let Some(codec) = text_codec {
        let ops = codec.diff(b"hello\n", b"modified on feature\n").unwrap();
        let patch = Patch {
            target_path: "file.txt".to_string(),
            codec_id: codec.id().to_string(),
            base_object: Some(file_blob_id),
            result_object: Some(modified_blob_id),
            ops,
            codec_payload: None,
        };
        let patch_id = store.store_object(&Object::Patch(patch)).unwrap();
        patches.push(patch_id);
    }

    let rev2 = Object::Revision(Revision {
        change_id: None,
        parents: vec![rev1_id],
        patches,
        snapshot_base: None,
        tree: Some(feature_tree_id),
        capsule_id: None,
        author: "test".to_string(),
        created_at_ms: 2000,
        summary: "change on feature".to_string(),
        policy_evidence: vec![],
    });
    let rev2_id = store.store_object(&rev2).unwrap();
    store.update_ref_cas("heads/feature", Some(&rev1_id), &rev2_id, "test", "feature commit").unwrap();

    // === Phase 5: Checkout main ===
    store.write_head(&HeadState::Symbolic {
        ref_name: "heads/main".to_string(),
    }).unwrap();

    // Verify main still at rev1
    let main_id = store.get_ref("heads/main").unwrap().unwrap();
    assert_eq!(main_id, rev1_id);

    // === Phase 6: Integrate feature into main ===
    let merge_result = merge(
        &store,
        &registry,
        &main_id,
        &rev2_id,
        "test",
        "Merge feature into main",
    ).unwrap();

    assert!(merge_result.conflicts.is_empty(), "merge should have no conflicts");
    assert_eq!(merge_result.revision.parents.len(), 2, "merge revision should have 2 parents");
    assert_eq!(merge_result.revision.parents[0], rev1_id);
    assert_eq!(merge_result.revision.parents[1], rev2_id);

    // Store merge revision and update main ref
    let merge_rev_id = store.store_object(&Object::Revision(merge_result.revision)).unwrap();
    store.update_ref_cas("heads/main", Some(&rev1_id), &merge_rev_id, "test", "merge").unwrap();

    // === Phase 7: Verify final state ===
    let merge_obj = store.load_object(&merge_rev_id).unwrap();
    let merge_tree_id = match merge_obj {
        Object::Revision(ref rev) => rev.tree.unwrap(),
        _ => panic!("expected revision"),
    };

    // Verify the merged tree contains the feature change
    let final_changes = diff_trees(&store, Some(&root_tree_id), Some(&merge_tree_id), "").unwrap();
    assert_eq!(final_changes.len(), 1, "merged tree should differ from initial by 1 file");
    assert_eq!(final_changes[0].path, "file.txt");

    // Load the merged file.txt blob and verify content
    let merged_tree_obj = store.load_object(&merge_tree_id).unwrap();
    if let Object::Tree(tree) = merged_tree_obj {
        let file_entry = tree.entries.iter().find(|e| e.name == "file.txt").unwrap();
        let blob_obj = store.load_object(&file_entry.object_id).unwrap();
        if let Object::Blob(b) = blob_obj {
            assert_eq!(
                String::from_utf8_lossy(&b.data),
                "modified on feature\n",
                "merged file.txt should have feature content"
            );
        } else {
            panic!("expected blob");
        }

        // Verify other files are unchanged
        let json_entry = tree.entries.iter().find(|e| e.name == "data.json").unwrap();
        assert_eq!(json_entry.object_id, json_blob_id, "data.json should be unchanged");

        let src_entry = tree.entries.iter().find(|e| e.name == "src").unwrap();
        assert_eq!(src_entry.object_id, src_tree_id, "src/ should be unchanged");
    } else {
        panic!("expected tree");
    }

    // Verify log: walk from merge back to initial
    let mut current = Some(merge_rev_id);
    let mut log_ids = vec![];
    while let Some(id) = current {
        log_ids.push(id);
        let obj = store.load_object(&id).unwrap();
        if let Object::Revision(rev) = obj {
            current = rev.parents.first().copied();
        } else {
            break;
        }
    }
    assert_eq!(log_ids.len(), 2, "log should have 2 entries (merge + initial)");
    assert_eq!(log_ids[0], merge_rev_id);
    assert_eq!(log_ids[1], rev1_id);

    // Verify branch refs
    let main_final = store.get_ref("heads/main").unwrap().unwrap();
    assert_eq!(main_final, merge_rev_id, "main should point to merge revision");
    let feature_final = store.get_ref("heads/feature").unwrap().unwrap();
    assert_eq!(feature_final, rev2_id, "feature should still point to its own revision");
}
