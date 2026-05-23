mod support;

use std::fs;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Command;

use claw_core::hash::content_hash;
use claw_core::object::{Object, TypeTag};
use claw_core::types::{Blob, FileMode, Revision, Tree, TreeEntry};
use claw_store::ClawStore;
use claw_sync::client::SyncClient;
use claw_sync::compat::{compatibility_report, CompatibilityLevel};
use claw_sync::proto::sync::sync_service_server::{SyncService, SyncServiceServer};
use claw_sync::proto::sync::{
    AdvertiseRefsRequest, AdvertiseRefsResponse, FetchObjectsRequest, HelloRequest, HelloResponse,
    ObjectChunk, PushObjectsResponse, UpdateRefsRequest, UpdateRefsResponse,
};
use support::CliTestEnv;
use tokio::sync::oneshot;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Server;
use tonic::{Request, Response, Status};

fn large_repo_file(repo: &Path, index: usize) -> PathBuf {
    repo.join("modules")
        .join(format!("{:02}", index % 12))
        .join(format!("component_{index:03}"))
        .join(format!("file_{index:03}.txt"))
}

#[test]
fn large_repo_synthetic_snapshot_status_and_checkout_scale() {
    const FILES: usize = 144;
    let env = CliTestEnv::new();
    let repo = env.init_repo("large-synthetic");

    for index in 0..FILES {
        env.write_file(
            &large_repo_file(&repo, index),
            &format!("component {index}\nline two\n"),
        );
    }

    let initial_status = env.run_ok(&repo, ["status", "--json"]);
    let initial_changes = initial_status.stdout_json()["changes"]
        .as_array()
        .expect("changes array")
        .clone();
    assert_eq!(initial_changes.len(), FILES);
    assert!(initial_changes
        .iter()
        .any(|change| change["path"] == "modules/00/component_000/file_000.txt"));

    env.run_ok(&repo, ["snapshot", "-m", "Synthetic large repo baseline"]);
    let clean = env.run_ok(&repo, ["status", "--json"]);
    assert_eq!(
        clean.stdout_json()["changes"]
            .as_array()
            .expect("clean changes")
            .len(),
        0
    );

    env.run_ok(&repo, ["branch", "create", "synthetic-edit"]);
    env.run_ok(&repo, ["checkout", "synthetic-edit"]);

    for index in 0..12 {
        env.write_file(
            &large_repo_file(&repo, index),
            &format!("component {index}\nmodified on feature\n"),
        );
    }
    for index in 20..26 {
        fs::remove_file(large_repo_file(&repo, index)).expect("delete synthetic file");
    }
    for index in 0..7 {
        env.write_file(
            &repo
                .join("new")
                .join(format!("fanout_{index:02}"))
                .join("added.txt"),
            &format!("new file {index}\n"),
        );
    }

    let dirty = env.run_ok(&repo, ["status", "--json"]);
    let dirty_json = dirty.stdout_json();
    let dirty_changes = dirty_json["changes"].as_array().expect("dirty changes");
    assert_eq!(dirty_changes.len(), 25);
    assert!(dirty_changes
        .iter()
        .any(|change| change["path"] == "new/fanout_00/added.txt" && change["status"] == "added"));
    assert!(dirty_changes.iter().any(|change| change["path"]
        == "modules/08/component_020/file_020.txt"
        && change["status"] == "deleted"));
    assert!(dirty_changes.iter().any(|change| change["path"]
        == "modules/00/component_000/file_000.txt"
        && change["status"] == "modified"));

    env.run_ok(&repo, ["snapshot", "-m", "Synthetic large repo edit"]);

    env.run_ok(&repo, ["checkout", "main"]);
    assert_eq!(
        env.read_file(&large_repo_file(&repo, 0)),
        "component 0\nline two\n"
    );
    assert!(large_repo_file(&repo, 20).exists());
    assert!(!repo
        .join("new")
        .join("fanout_00")
        .join("added.txt")
        .exists());

    env.run_ok(&repo, ["checkout", "synthetic-edit"]);
    assert_eq!(
        env.read_file(&large_repo_file(&repo, 0)),
        "component 0\nmodified on feature\n"
    );
    assert!(!large_repo_file(&repo, 20).exists());
    assert!(repo
        .join("new")
        .join("fanout_00")
        .join("added.txt")
        .exists());
}

#[test]
fn large_repo_varied_fixture_diff_and_path_filter_scale() {
    const FILES: usize = 192;
    let env = CliTestEnv::new();
    let repo = env.init_repo("large-varied-fixtures");

    for index in 0..FILES {
        env.write_file(
            &large_repo_file(&repo, index),
            &format!("component {index}\nline two\n"),
        );
    }

    let large_json = format!(
        "{{\"items\":[{}]}}\n",
        (0..1024)
            .map(|idx| format!("{{\"id\":{idx},\"value\":\"v{idx}\"}}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    env.write_file(&repo.join("fixtures").join("large.json"), &large_json);
    fs::write(
        repo.join("fixtures").join("large.bin"),
        (0..(256 * 1024))
            .map(|idx| (idx % 251) as u8)
            .collect::<Vec<_>>(),
    )
    .expect("write large binary fixture");

    let initial_status = env.run_ok(&repo, ["status", "--json"]);
    assert_eq!(
        initial_status.stdout_json()["changes"]
            .as_array()
            .expect("initial varied changes")
            .len(),
        FILES + 2
    );

    env.run_ok(&repo, ["snapshot", "-m", "Varied large repo baseline"]);

    env.write_file(
        &large_repo_file(&repo, 191),
        "component 191\npath-filter regression\n",
    );
    env.write_file(
        &repo.join("fixtures").join("large.json"),
        &large_json.replace("\"v1023\"", "\"changed\""),
    );
    fs::write(
        repo.join("fixtures").join("large.bin"),
        vec![0xa5; 256 * 1024],
    )
    .expect("rewrite large binary fixture");

    let all_diff = env.run_ok(&repo, ["diff", "--json"]);
    let all_diff_json = all_diff.stdout_json();
    assert_eq!(all_diff_json["schema_version"], 1);
    assert_eq!(all_diff_json["action"], "diff");
    assert_eq!(all_diff_json["from"], "HEAD");
    assert_eq!(all_diff_json["to"], "working_tree");
    assert!(all_diff_json["path_filter"].is_null());
    assert_eq!(all_diff_json["change_count"], 3);
    let all_changes = all_diff_json["changes"]
        .as_array()
        .expect("all varied diff changes")
        .clone();
    assert_eq!(all_changes.len(), 3);
    assert!(all_changes
        .iter()
        .any(|change| change["path"] == "modules/11/component_191/file_191.txt"));

    let fixture_diff = env.run_ok(&repo, ["diff", "--json", "--path", "fixtures"]);
    let fixture_diff_json = fixture_diff.stdout_json();
    assert_eq!(fixture_diff_json["schema_version"], 1);
    assert_eq!(fixture_diff_json["action"], "diff");
    assert_eq!(fixture_diff_json["from"], "HEAD");
    assert_eq!(fixture_diff_json["to"], "working_tree");
    assert_eq!(fixture_diff_json["path_filter"], "fixtures");
    assert_eq!(fixture_diff_json["change_count"], 2);
    let fixture_changes = fixture_diff_json["changes"]
        .as_array()
        .expect("fixture path filtered changes")
        .clone();
    assert_eq!(fixture_changes.len(), 2);
    assert!(fixture_changes.iter().all(|change| change["path"]
        .as_str()
        .is_some_and(|path| path.starts_with("fixtures/"))));
    assert!(fixture_changes
        .iter()
        .any(|change| change["path"] == "fixtures/large.json"));
    assert!(fixture_changes
        .iter()
        .any(|change| change["path"] == "fixtures/large.bin"));

    let fixture_names = env.run_ok(&repo, ["diff", "--name-only", "--path", "fixtures"]);
    assert!(fixture_names.stdout.contains("M fixtures/large.json"));
    assert!(fixture_names.stdout.contains("M fixtures/large.bin"));
    assert!(!fixture_names.stdout.contains("component_191"));
}

#[test]
#[ignore = "10k-file large-repo drill is intentionally operator-triggered"]
fn large_repo_10k_file_snapshot_status_and_path_filter_drill() {
    const FILES: usize = 10_000;
    let env = CliTestEnv::new();
    let repo = env.init_repo("large-10k-synthetic");

    for index in 0..FILES {
        env.write_file(
            &large_repo_file(&repo, index),
            &format!("component {index}\nline two\n"),
        );
    }
    env.write_file(
        &repo.join("fixtures").join("large.json"),
        &format!(
            "{{\"items\":[{}]}}\n",
            (0..2048)
                .map(|idx| format!("{{\"id\":{idx},\"value\":\"v{idx}\"}}"))
                .collect::<Vec<_>>()
                .join(",")
        ),
    );
    fs::create_dir_all(repo.join("fixtures")).expect("create fixtures directory");
    fs::write(
        repo.join("fixtures").join("large.bin"),
        vec![0x5au8; 1024 * 1024],
    )
    .expect("write large binary fixture");

    let status = env.run_ok(&repo, ["status", "--json"]);
    let status_json = status.stdout_json();
    let changes = status_json["changes"]
        .as_array()
        .expect("large repo status changes");
    assert_eq!(changes.len(), FILES + 2);

    env.run_ok(&repo, ["snapshot", "-m", "10k synthetic baseline"]);
    env.write_file(
        &large_repo_file(&repo, 9_999),
        "component 9999\npath-filter drill\n",
    );

    let dirty = env.run_ok(&repo, ["status", "--json"]);
    let dirty_json = dirty.stdout_json();
    let dirty_changes = dirty_json["changes"]
        .as_array()
        .expect("large repo dirty changes");
    assert_eq!(dirty_changes.len(), 1);
    assert_eq!(
        dirty_changes[0]["path"],
        "modules/03/component_9999/file_9999.txt"
    );
}

#[test]
fn admin_backup_verify_and_rollback_restore_corrupted_metadata() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("disaster-recovery");

    env.write_file(&repo.join("src").join("main.rs"), "fn main() {}\n");
    env.run_ok(&repo, ["snapshot", "-m", "Recoverable baseline"]);

    let created = env.run_ok(&repo, ["admin", "--json", "backup", "create"]);
    let created_json = created.stdout_json();
    assert_eq!(created_json["schema_version"], 1);
    assert_eq!(created_json["action"], "backup.create");
    assert!(created_json["file_count"].as_u64().unwrap_or_default() > 0);
    let backup_id = created_json["backup_id"]
        .as_str()
        .expect("backup id")
        .to_string();
    let verified = env.run_ok(
        &repo,
        [
            "admin",
            "--json",
            "backup",
            "verify",
            "--backup-id",
            backup_id.as_str(),
        ],
    );
    let verified_json = verified.stdout_json();
    assert_eq!(verified_json["schema_version"], 1);
    assert_eq!(verified_json["action"], "backup.verify");
    assert_eq!(verified_json["backup_id"], backup_id);
    assert_eq!(verified_json["backup"]["backup_id"], backup_id);
    assert_eq!(verified_json["backup"]["verified"], true);

    let main_ref = repo.join(".claw").join("refs").join("heads").join("main");
    let original_ref = fs::read_to_string(&main_ref).expect("read original main ref");
    fs::write(
        &main_ref,
        "0000000000000000000000000000000000000000000000000000000000000000\n",
    )
    .expect("corrupt main ref");
    let stray_ref = repo.join(".claw").join("refs").join("heads").join("stray");
    fs::write(&stray_ref, "not-a-real-ref\n").expect("write stray ref");

    let plan = env.run_ok(
        &repo,
        [
            "admin",
            "--json",
            "rollback",
            "plan",
            "--backup-id",
            backup_id.as_str(),
        ],
    );
    let plan_json = plan.stdout_json();
    assert_eq!(plan_json["schema_version"], 1);
    assert_eq!(plan_json["action"], "rollback.plan");
    assert_eq!(plan_json["backup_id"], backup_id);
    assert_eq!(plan_json["verified"], true);
    assert!(plan_json["restore_file_count"].as_u64().unwrap_or_default() > 0);

    let executed = env.run_ok(
        &repo,
        [
            "admin",
            "--json",
            "rollback",
            "execute",
            "--backup-id",
            backup_id.as_str(),
        ],
    );
    let executed_json = executed.stdout_json();
    assert_eq!(executed_json["schema_version"], 1);
    assert_eq!(executed_json["action"], "rollback.execute");
    assert_eq!(executed_json["backup_id"], backup_id);
    assert_eq!(executed_json["verified"], true);
    assert_eq!(executed_json["restored"], true);

    assert_eq!(
        fs::read_to_string(&main_ref).expect("read restored main ref"),
        original_ref
    );
    assert!(
        !stray_ref.exists(),
        "rollback should remove metadata files absent from the backup snapshot"
    );

    env.run_ok(
        &repo,
        [
            "admin",
            "backup",
            "verify",
            "--backup-id",
            backup_id.as_str(),
        ],
    );
    let head = env.run_ok(&repo, ["show", "heads/main"]);
    assert!(head.stdout.contains("Recoverable baseline"));
}

#[test]
fn git_notes_export_and_import_roundtrip_policy_evidence() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("git-notes-interop");
    run_git_ok(&repo, &["init", "-q"]);

    let original_revision = seed_policy_evidence_revision(&repo);
    let exported = env.run_ok(
        &repo,
        [
            "git-export",
            "--git-notes",
            "--notes-ref",
            "claw-provenance",
        ],
    );
    assert!(exported
        .stdout
        .contains("wrote 1 provenance note(s) to refs/notes/claw-provenance"));
    let commit_hex = exported.value_after("SHA-1: ");

    let note = run_git_ok(
        &repo,
        &[
            "--git-dir",
            ".git",
            "notes",
            "--ref",
            "claw-provenance",
            "show",
            commit_hex.as_str(),
        ],
    );
    let note_json: serde_json::Value = serde_json::from_str(&note).expect("git note json");
    assert_eq!(note_json["revision_id"], original_revision);
    assert_eq!(note_json["policy_evidence"][0], "ci/git-notes=pass");

    let imported = env.run_ok(
        &repo,
        [
            "git-import",
            "--read-notes",
            "--notes-ref",
            "claw-provenance",
            "--git-ref",
            "refs/heads/claw/main",
            "--ref-name",
            "heads/imported-with-notes",
        ],
    );
    assert!(imported
        .stdout
        .contains("Imported 1 provenance note(s) from refs/notes/claw-provenance"));
    let imported_revision = imported.value_after("Revision: ");

    let store = ClawStore::open(&repo).expect("open store after git import");
    let evidence_ref = format!("notes/provenance/policy-evidence/{imported_revision}");
    let evidence_blob_id = store
        .get_ref(&evidence_ref)
        .expect("read imported evidence ref")
        .expect("evidence ref should exist");
    let evidence = match store
        .load_object(&evidence_blob_id)
        .expect("load evidence blob")
    {
        Object::Blob(blob) => {
            serde_json::from_slice::<Vec<String>>(&blob.data).expect("evidence json")
        }
        other => panic!("expected evidence blob, got {other:?}"),
    };
    assert_eq!(
        evidence,
        vec![
            "ci/git-notes=pass".to_string(),
            "artifact/provenance=present".to_string()
        ]
    );
}

#[test]
fn cli_dx_json_and_policy_dry_run_surfaces_are_machine_readable() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("cli-dx-json");

    let version = env.run_ok(env.temp_root(), ["version", "--json"]);
    let version_json = version.stdout_json();
    assert_eq!(version_json["schema_version"], 1);
    assert_eq!(version_json["action"], "version");
    assert_eq!(version_json["object_format_version"], 1);
    assert_eq!(version_json["sync_protocol_version"], "claw-sync/1");
    assert!(version_json["build"]["target"].as_str().is_some());

    let doctor = env.run_ok(&repo, ["doctor", "--json"]);
    let doctor_json = doctor.stdout_json();
    assert_eq!(doctor_json["schema_version"], 1);
    assert_eq!(doctor_json["action"], "doctor");
    let check_names: Vec<_> = doctor_json["checks"]
        .as_array()
        .expect("doctor checks")
        .iter()
        .filter_map(|check| check["name"].as_str())
        .collect();
    assert!(check_names.contains(&"git"));
    assert!(check_names.contains(&"object_format"));
    assert!(check_names.contains(&"refs"));
    assert!(check_names.contains(&"daemon_auth"));

    let repair_plan = env.run_ok(&repo, ["repair", "--json", "plan"]);
    let repair_plan_json = repair_plan.stdout_json();
    assert_eq!(repair_plan_json["schema_version"], 1);
    assert_eq!(repair_plan_json["action"], "repair.plan");
    assert!(repair_plan_json["object_count"].as_u64().is_some());
    assert!(repair_plan_json["ref_count"].as_u64().is_some());
    assert!(repair_plan_json["issue_count"].as_u64().is_some());
    assert!(repair_plan_json["repairable_count"].as_u64().is_some());
    assert!(repair_plan_json["summary"]["error_count"]
        .as_u64()
        .is_some());
    assert!(repair_plan_json["summary"]["invalid_ref_namespace_count"]
        .as_u64()
        .is_some());
    assert!(repair_plan_json["issues"].as_array().is_some());

    let repair_apply = env.run_ok(&repo, ["repair", "--json", "apply", "--dry-run"]);
    let repair_apply_json = repair_apply.stdout_json();
    assert_eq!(repair_apply_json["schema_version"], 1);
    assert_eq!(repair_apply_json["action"], "repair.apply");
    assert_eq!(repair_apply_json["dry_run"], true);
    assert!(repair_apply_json["planned_count"].as_u64().is_some());
    assert_eq!(repair_apply_json["applied_count"], 0);
    assert!(repair_apply_json["summary"]["error_count"]
        .as_u64()
        .is_some());
    assert!(repair_apply_json["summary"]["invalid_ref_namespace_count"]
        .as_u64()
        .is_some());
    assert!(repair_apply_json["post_summary"].is_null());
    assert!(repair_apply_json["remaining_issue_count"].is_null());
    assert!(repair_apply_json["remaining_repairable_count"].is_null());
    assert!(repair_apply_json["planned"].as_array().is_some());
    assert!(repair_apply_json["applied"].as_array().is_some());

    let dry_run = env.run_ok(
        &repo,
        [
            "policy",
            "apply",
            "--id",
            "release",
            "--check",
            "ci",
            "--dry-run",
            "--json",
        ],
    );
    let dry_json = dry_run.stdout_json();
    assert_eq!(dry_json["schema_version"], 1);
    assert_eq!(dry_json["action"], "policy.apply");
    assert_eq!(dry_json["dry_run"], true);
    assert_eq!(dry_json["ref"], "policies/release");
    let store = ClawStore::open(&repo).expect("open repo after policy dry-run");
    assert!(
        store
            .get_ref("policies/release")
            .expect("read policy ref")
            .is_none(),
        "policy apply --dry-run must not write the policy ref"
    );

    env.run_ok(
        &repo,
        ["policy", "create", "--id", "release", "--check", "ci"],
    );
    let shown = env.run_ok(&repo, ["show", "--json", "policies/release"]);
    let shown_json = shown.stdout_json();
    assert_eq!(shown_json["schema_version"], 1);
    assert_eq!(shown_json["action"], "show");
    assert_eq!(shown_json["query"], "policies/release");
    assert_eq!(shown_json["type"], "policy");
    assert!(shown_json["object_id"]
        .as_str()
        .expect("policy object display id")
        .starts_with("clw_"));
    assert_eq!(shown_json["object"]["id"], shown_json["object_id"]);
    assert_eq!(shown_json["object"]["hex"], shown_json["object_hex"]);
    assert_eq!(shown_json["object"]["type"], shown_json["type"]);
    assert_eq!(shown_json["object"]["value"], shown_json["value"]);
    assert_eq!(shown_json["object"]["type"], "policy");
    assert_eq!(
        shown_json["object"]["value"]["Policy"]["policy_id"],
        "release"
    );
}

#[test]
fn doctor_reports_non_portable_manual_ref_files() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("doctor-invalid-ref-namespace");
    env.write_file(&repo.join("README.md"), "hello\n");
    env.run_ok(&repo, ["snapshot", "-m", "initial"]);

    let main_ref = repo.join(".claw").join("refs").join("heads").join("main");
    let invalid_ref = repo.join(".claw").join("refs").join("heads").join("CON");
    fs::write(
        &invalid_ref,
        fs::read_to_string(&main_ref).expect("read main ref"),
    )
    .expect("write invalid manual ref");

    let doctor = env.run_fail(&repo, ["doctor", "--json", "--strict"]);
    let doctor_json = doctor.stdout_json();
    let refs_check = doctor_json["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .find(|check| check["name"] == "refs")
        .expect("refs check");

    assert_eq!(refs_check["status"], "error");
    assert!(refs_check["message"]
        .as_str()
        .expect("refs message")
        .contains("heads/CON"));
    assert!(refs_check["remediation"]
        .as_str()
        .expect("refs remediation")
        .contains(".claw/refs"));

    let deep_doctor = env.run_fail(&repo, ["doctor", "--json", "--deep", "--strict"]);
    let deep_doctor_json = deep_doctor.stdout_json();
    assert_eq!(
        deep_doctor_json["deep"]["summary"]["invalid_ref_namespace_count"],
        1
    );
    assert!(deep_doctor_json["deep"]["issues"]
        .as_array()
        .expect("deep issues")
        .iter()
        .any(|issue| issue["code"] == "invalid_ref_namespace" && issue["repair"].is_null()));

    let repair_plan = env.run_ok(&repo, ["repair", "--json", "plan"]);
    let repair_plan_json = repair_plan.stdout_json();
    assert_eq!(
        repair_plan_json["summary"]["invalid_ref_namespace_count"],
        1
    );
    assert!(repair_plan_json["issues"]
        .as_array()
        .expect("repair issues")
        .iter()
        .any(|issue| issue["code"] == "invalid_ref_namespace"
            && issue["message"]
                .as_str()
                .is_some_and(|message| message.contains("heads/CON"))
            && issue["repair"].is_null()));
}

#[test]
fn doctor_deep_reports_weak_agent_keys() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("doctor-weak-agent-key");
    let store = ClawStore::open(&repo).expect("open weak agent repo");
    let weak_agent = serde_json::json!({
        "schema_version": 2,
        "agent_id": "weak-agent",
        "public_key": "0000000000000000000000000000000000000000000000000000000000000000",
        "created_at_ms": 1,
        "updated_at_ms": 1
    });
    let weak_agent_id = store
        .store_object(&Object::Blob(Blob {
            data: serde_json::to_vec(&weak_agent).expect("weak agent json"),
            media_type: Some("application/json".to_string()),
        }))
        .expect("store weak agent registration");
    store
        .set_ref("agents/weak-agent", &weak_agent_id)
        .expect("set weak agent ref");

    let doctor = env.run_fail(&repo, ["doctor", "--json", "--deep", "--strict"]);
    let doctor_json = doctor.stdout_json();
    assert_eq!(doctor_json["schema_version"], 1);
    assert_eq!(doctor_json["action"], "doctor");
    assert_eq!(doctor_json["deep"]["summary"]["weak_key_count"], 1);
    assert_eq!(doctor_json["deep"]["summary"]["error_count"], 1);
    assert_eq!(
        doctor_json["deep"]["summary"]["invalid_ref_namespace_count"],
        0
    );
    assert_eq!(doctor_json["deep"]["summary"]["policy_drift_count"], 0);
    assert_eq!(doctor_json["deep"]["summary"]["missing_capsule_count"], 0);
    assert_eq!(doctor_json["deep"]["summary"]["dangling_ref_count"], 0);
    assert!(doctor_json["deep"]["issues"]
        .as_array()
        .expect("deep health issues")
        .iter()
        .any(|issue| issue["code"] == "agent_weak_public_key"
            && issue["ref_name"] == "agents/weak-agent"));

    let repair_plan = env.run_ok(&repo, ["repair", "--json", "plan"]);
    let repair_plan_json = repair_plan.stdout_json();
    assert!(repair_plan_json["issues"]
        .as_array()
        .expect("repair plan issues")
        .iter()
        .any(|issue| issue["code"] == "agent_weak_public_key" && issue["repair"].is_null()));
}

#[test]
fn repair_apply_rolls_back_refs_and_regenerates_policy_audit_metadata() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("repair-safe-actions");

    env.run_ok(
        &repo,
        ["policy", "create", "--id", "release", "--check", "test"],
    );
    let intent = env.run_ok(
        &repo,
        [
            "intent",
            "create",
            "--title",
            "Repair stale audit data",
            "--goal",
            "Verify safe repair actions",
        ],
    );
    let intent_id = intent.value_after("Created intent: ");
    env.run_ok(
        &repo,
        ["intent", "policy", "add", intent_id.as_str(), "release"],
    );

    let store = ClawStore::open(&repo).expect("open repair test repo");
    let intent_ref = format!("intents/{intent_id}");
    let intent_obj_id = store
        .get_ref(&intent_ref)
        .expect("read intent ref")
        .expect("intent ref exists");
    let Object::Intent(mut intent) = store.load_object(&intent_obj_id).expect("load intent") else {
        panic!("intent ref should point to intent object");
    };
    intent.policy_refs.push("missing-policy".to_string());
    let updated_intent_id = store
        .store_object(&Object::Intent(intent))
        .expect("store stale intent policy ref");
    store
        .set_ref(&intent_ref, &updated_intent_id)
        .expect("update intent ref");

    let valid_ref_target = store
        .store_object(&Object::Blob(Blob {
            data: b"valid".to_vec(),
            media_type: None,
        }))
        .expect("store valid ref target");
    store
        .set_ref("heads/repair-target", &valid_ref_target)
        .expect("seed valid ref target");
    let missing_ref_target = content_hash(TypeTag::Blob, b"missing repair target");
    store
        .update_ref_cas(
            "heads/repair-target",
            Some(&valid_ref_target),
            &missing_ref_target,
            "test",
            "simulate broken ref",
        )
        .expect("write broken ref with reflog");
    store
        .delete_ref("policies/release")
        .expect("delete policy to create stale intent policy ref");
    drop(store);

    let plan = env.run_ok(&repo, ["repair", "--json", "plan"]);
    let plan_json = plan.stdout_json();
    assert!(plan_json["repairable_count"].as_u64().unwrap_or(0) >= 2);
    let repair_kinds = plan_json["issues"]
        .as_array()
        .expect("repair issues")
        .iter()
        .filter_map(|issue| issue["repair"]["kind"].as_str())
        .collect::<Vec<_>>();
    assert!(repair_kinds.contains(&"ref_rollback"));
    assert!(repair_kinds.contains(&"ref_recovery"));
    assert!(repair_kinds.contains(&"policy_audit_regeneration"));

    let dry_run = env.run_ok(&repo, ["repair", "--json", "apply", "--dry-run"]);
    let dry_run_json = dry_run.stdout_json();
    assert_eq!(dry_run_json["dry_run"], true);
    assert_eq!(dry_run_json["applied_count"], 0);
    assert!(dry_run_json["planned_count"].as_u64().unwrap_or(0) >= 2);
    assert!(
        dry_run_json["summary"]["repairable_count"]
            .as_u64()
            .unwrap_or(0)
            >= 2
    );
    assert!(dry_run_json["post_summary"].is_null());
    assert!(dry_run_json["remaining_issue_count"].is_null());
    assert!(dry_run_json["remaining_repairable_count"].is_null());

    let applied = env.run_ok(&repo, ["repair", "--json", "apply"]);
    let applied_json = applied.stdout_json();
    assert!(applied_json["applied_count"].as_u64().unwrap_or(0) >= 2);
    assert!(
        applied_json["summary"]["repairable_count"]
            .as_u64()
            .unwrap_or(0)
            >= 2
    );
    assert_eq!(applied_json["remaining_issue_count"], 0);
    assert_eq!(applied_json["remaining_repairable_count"], 0);
    assert_eq!(
        applied_json["post_summary"]["repairable_count"], 0,
        "post-apply summary should describe remaining repair work"
    );

    let store = ClawStore::open(&repo).expect("reopen repaired repo");
    assert_eq!(
        store
            .get_ref("heads/repair-target")
            .expect("read repaired ref"),
        Some(valid_ref_target)
    );
    let intent_obj = store
        .get_ref(&intent_ref)
        .expect("read repaired intent ref")
        .expect("intent ref exists");
    let Object::Intent(intent) = store
        .load_object(&intent_obj)
        .expect("load repaired intent")
    else {
        panic!("intent ref should point to intent object");
    };
    assert!(
        intent.policy_refs.contains(&"release".to_string()),
        "recoverable policy refs should stay attached"
    );
    assert!(
        !intent.policy_refs.contains(&"missing-policy".to_string()),
        "unrecoverable stale policy refs should be pruned"
    );
}

#[test]
fn repair_recovers_deleted_ref_from_surviving_reflog() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("repair-deleted-ref-reflog");

    let store = ClawStore::open(&repo).expect("open reflog recovery repo");
    let target = store
        .store_object(&Object::Blob(Blob {
            data: b"recoverable ref target".to_vec(),
            media_type: None,
        }))
        .expect("store recoverable target");
    store
        .update_ref_cas(
            "heads/recover-me",
            None,
            &target,
            "test",
            "seed recoverable ref",
        )
        .expect("write ref with reflog");
    store
        .delete_ref("heads/recover-me")
        .expect("delete ref while leaving reflog");
    drop(store);

    let plan = env.run_ok(&repo, ["repair", "--json", "plan"]);
    let plan_json = plan.stdout_json();
    assert!(plan_json["issues"]
        .as_array()
        .expect("repair issues")
        .iter()
        .any(|issue| issue["code"] == "deleted_ref_with_reflog"
            && issue["ref_name"] == "heads/recover-me"
            && issue["repair"]["kind"] == "ref_recovery"
            && issue["repair"]["target"] == target.to_hex()));

    let applied = env.run_ok(&repo, ["repair", "apply"]);
    assert!(applied.stdout.contains("Applied 1 safe repair(s)."));
    assert!(applied
        .stdout
        .contains("ref_recovery: restore heads/recover-me"));
    assert!(applied
        .stdout
        .contains("Remaining after repair: 0 issue(s), 0 safe repair(s)."));

    let store = ClawStore::open(&repo).expect("reopen recovered repo");
    assert_eq!(
        store
            .get_ref("heads/recover-me")
            .expect("read recovered ref"),
        Some(target)
    );
}

#[test]
fn repair_rebuilds_short_capsule_index_ref() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("repair-short-capsule-index");

    let intent = env.run_ok(
        &repo,
        [
            "intent",
            "create",
            "--title",
            "Repair capsule index",
            "--goal",
            "Rebuild short capsule lookup refs",
        ],
    );
    let intent_id = intent.value_after("Created intent: ");
    let change = env.run_ok(&repo, ["change", "create", "--intent", intent_id.as_str()]);
    let change_id = change.value_after("Created change: ");

    env.write_file(
        &repo.join("src/lib.rs"),
        "pub fn repaired() -> bool { true }\n",
    );
    env.run_ok(
        &repo,
        [
            "snapshot",
            "-m",
            "index repair target",
            "--change",
            change_id.as_str(),
        ],
    );
    env.run_ok(&repo, ["ship", "--intent", intent_id.as_str()]);

    let store = ClawStore::open(&repo).expect("open capsule index repo");
    let revision_id = store
        .get_ref("heads/main")
        .expect("read main ref")
        .expect("main ref exists");
    let capsule_id = store
        .get_ref(&format!("capsules/by-revision/{}", revision_id.to_hex()))
        .expect("read full capsule index")
        .expect("full capsule index exists");
    let short_ref = format!("capsules/by-revision/{}", &revision_id.to_hex()[..16]);
    assert_eq!(
        store.get_ref(&short_ref).expect("read short capsule index"),
        Some(capsule_id)
    );
    store
        .delete_ref(&short_ref)
        .expect("delete short capsule index");
    drop(store);

    let plan = env.run_ok(&repo, ["repair", "--json", "plan"]);
    let plan_json = plan.stdout_json();
    assert!(plan_json["issues"]
        .as_array()
        .expect("repair issues")
        .iter()
        .any(|issue| issue["code"] == "capsule_index_drift"
            && issue["ref_name"] == short_ref
            && issue["repair"]["kind"] == "capsule_reindex"));

    let applied = env.run_ok(&repo, ["repair", "--json", "apply"]);
    let applied_json = applied.stdout_json();
    assert!(applied_json["applied"]
        .as_array()
        .expect("applied repairs")
        .iter()
        .any(|repair| repair["kind"] == "capsule_reindex" && repair["ref_name"] == short_ref));

    let store = ClawStore::open(&repo).expect("reopen capsule index repo");
    assert_eq!(
        store.get_ref(&short_ref).expect("read rebuilt short index"),
        Some(capsule_id)
    );
}

#[test]
fn git_export_dry_run_skips_git_writes() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("git-export-dry-run");
    env.write_file(&repo.join("hello.txt"), "hello\n");
    env.run_ok(&repo, ["snapshot", "-m", "initial"]);

    let git_dir = repo.join("exported.git");
    let dry_run = env.run_ok(
        &repo,
        [
            "git-export",
            "--json",
            "--git-dir",
            git_dir.to_str().expect("git dir utf-8"),
            "--dry-run",
        ],
    );

    let dry_run_json = dry_run.stdout_json();
    assert_eq!(dry_run_json["schema_version"], 1);
    assert_eq!(dry_run_json["action"], "git-export");
    assert_eq!(dry_run_json["dry_run"], true);
    assert_eq!(dry_run_json["export_count"], 1);
    assert_eq!(dry_run_json["exports"][0]["source_ref"], "heads/main");
    assert_eq!(dry_run_json["exports"][0]["git_branch"], "claw/main");
    assert_eq!(dry_run_json["exports"][0]["revision_count"], 1);
    assert_eq!(
        dry_run_json["exports"][0]["git_commit"],
        serde_json::Value::Null
    );
    assert!(
        !git_dir.exists(),
        "git-export --dry-run must not create the target git directory"
    );
}

#[test]
fn git_import_dry_run_skips_claw_ref_writes() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("git-import-dry-run");
    let git_repo = env.repo_path("source-git");
    fs::create_dir_all(&git_repo).expect("create git source repo");
    run_git_ok(&git_repo, &["init", "-q"]);
    run_git_ok(&git_repo, &["config", "user.name", "Claw Tests"]);
    run_git_ok(&git_repo, &["config", "user.email", "tests@example.com"]);
    fs::write(git_repo.join("hello.txt"), "hello from git\n").expect("write git source file");
    run_git_ok(&git_repo, &["add", "hello.txt"]);
    run_git_ok(&git_repo, &["commit", "-q", "-m", "initial"]);
    run_git_ok(&git_repo, &["branch", "-M", "main"]);

    let dry_run = env.run_ok(
        &repo,
        [
            "git-import",
            "--json",
            "--git-dir",
            git_repo
                .join(".git")
                .to_str()
                .expect("git source path utf-8"),
            "--git-ref",
            "refs/heads/main",
            "--ref-name",
            "heads/imported",
            "--dry-run",
        ],
    );

    let dry_run_json = dry_run.stdout_json();
    assert_eq!(dry_run_json["schema_version"], 1);
    assert_eq!(dry_run_json["action"], "git-import");
    assert_eq!(dry_run_json["dry_run"], true);
    assert_eq!(dry_run_json["import_count"], 1);
    assert_eq!(dry_run_json["imports"][0]["git_ref"], "refs/heads/main");
    assert_eq!(dry_run_json["imports"][0]["claw_ref"], "heads/imported");
    assert_eq!(
        dry_run_json["imports"][0]["revision_id"],
        serde_json::Value::Null
    );
    let store = ClawStore::open(&repo).expect("open repo after git import dry-run");
    assert!(
        store
            .get_ref("heads/imported")
            .expect("read imported ref")
            .is_none(),
        "git-import --dry-run must not write the destination ref"
    );
}

#[test]
fn bridge_import_json_reports_schema_and_dry_run_refs() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("bridge-import-json-schema");

    env.write_file(&repo.join("README.md"), "bridge target\n");
    env.run_ok(&repo, ["snapshot", "-m", "Bridge import target"]);

    fs::write(
        repo.join("provider.json"),
        r#"{
  "pull_request": {
    "number": 17,
    "title": "Bridge provider metadata",
    "body": "Import hosted review context.",
    "state": "open",
    "html_url": "https://example.test/pulls/17",
    "head": {"ref": "feature/bridge"},
    "base": {"ref": "main"}
  },
  "branch_protection": {
    "name": "main",
    "required_status_checks": {"contexts": ["ci"]}
  }
}
"#,
    )
    .expect("write bridge provider metadata");

    let imported = env.run_ok(
        &repo,
        [
            "bridge",
            "--json",
            "import",
            "--provider",
            "GitHub",
            "--file",
            "provider.json",
            "--revision",
            "heads/main",
            "--dry-run",
        ],
    );
    let json = imported.stdout_json();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["action"], "bridge.import");
    assert_eq!(json["provider"], "github");
    assert_eq!(json["dry_run"], true);
    assert!(json["revision"].as_str().expect("revision hex").len() >= 32);
    assert!(json["raw_import_ref"]
        .as_str()
        .expect("raw import ref")
        .starts_with("bridges/github/imports/"));
    assert_eq!(json["intent"]["ref_name"], "bridges/github/pulls/17/intent");
    assert_eq!(json["change"]["ref_name"], "bridges/github/pulls/17/change");
    assert_eq!(json["policy"]["ref_name"], "policies/github-branch-main");
    assert_eq!(json["capsule"], serde_json::Value::Null);
    assert_eq!(json["evidence_added"], 0);
    assert_eq!(json["notes_imported"], 0);
    assert_eq!(json["mapping"]["pull_request_present"], true);
    assert_eq!(json["mapping"]["branch_protection_present"], true);
    assert_eq!(json["mapping"]["required_check_count"], 1);
    assert_eq!(
        json["mapping"]["required_approving_review_count"],
        serde_json::Value::Null
    );
    assert_eq!(json["mapping"]["require_code_owner_reviews"], false);

    let store = ClawStore::open(&repo).expect("open repo after bridge import dry-run");
    assert!(store
        .get_ref("bridges/github/pulls/17/intent")
        .expect("read dry-run bridge intent ref")
        .is_none());

    fs::write(
        repo.join("provider-with-evidence.json"),
        r#"{
  "pull_request": {
    "number": 18,
    "title": "Bridge provider evidence",
    "body": "Import hosted checks and reviews.",
    "state": "open",
    "html_url": "https://example.test/pulls/18",
    "head": {"ref": "feature/evidence"},
    "base": {"ref": "main"}
  },
  "checks": [{"name": "test", "conclusion": "success"}],
  "statuses": [{"context": "lint", "state": "success"}],
  "reviews": [{"user": {"login": "alice"}, "state": "APPROVED"}],
  "git_notes": [{"body": "legacy provenance note"}]
}
"#,
    )
    .expect("write bridge provider evidence metadata");

    let evidence_preview = env.run_ok(
        &repo,
        [
            "bridge",
            "--json",
            "import",
            "--provider",
            "github",
            "--file",
            "provider-with-evidence.json",
            "--revision",
            "heads/main",
            "--agent",
            "bridge-agent",
            "--dry-run",
        ],
    );
    let evidence_json = evidence_preview.stdout_json();
    assert_eq!(evidence_json["dry_run"], true);
    assert_eq!(evidence_json["evidence_added"], 4);
    assert_eq!(evidence_json["notes_imported"], 1);
    assert_eq!(evidence_json["mapping"]["check_count"], 1);
    assert_eq!(evidence_json["mapping"]["status_count"], 1);
    assert_eq!(evidence_json["mapping"]["review_count"], 1);
    assert_eq!(evidence_json["mapping"]["note_count"], 1);
    assert!(evidence_json["capsule"]["ref_name"]
        .as_str()
        .expect("dry-run bridge capsule ref")
        .starts_with("capsules/by-revision/"));
    assert!(
        store
            .get_ref("agents/bridge-agent")
            .expect("read dry-run bridge agent ref")
            .is_none(),
        "bridge import --dry-run must not register signing agents"
    );
    assert!(
        store
            .get_ref(
                evidence_json["capsule"]["ref_name"]
                    .as_str()
                    .expect("dry-run capsule ref name")
            )
            .expect("read dry-run bridge capsule ref")
            .is_none(),
        "bridge import --dry-run must not write capsule evidence refs"
    );
}

#[test]
fn remote_json_receipts_report_schema_and_dry_run_state() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("remote-json-schema");

    let dry_add = env.run_ok(
        &repo,
        [
            "remote",
            "--json",
            "add",
            "origin",
            "http://127.0.0.1:50051",
            "--kind",
            "grpc",
            "--token-profile",
            "ci",
            "--dry-run",
        ],
    );
    let dry_add_json = dry_add.stdout_json();
    assert_eq!(dry_add_json["schema_version"], 1);
    assert_eq!(dry_add_json["action"], "remote.add");
    assert_eq!(dry_add_json["dry_run"], true);
    assert_eq!(dry_add_json["saved"], false);
    assert_eq!(dry_add_json["remote"]["name"], "origin");
    assert_eq!(dry_add_json["remote"]["kind"], "grpc");
    assert_eq!(dry_add_json["remote"]["token_profile"], "ci");
    assert_eq!(dry_add_json["remote"]["capabilities"]["hosted_http"], false);
    assert_eq!(
        dry_add_json["remote"]["capabilities"]["partial_clone"],
        true
    );
    assert_eq!(
        dry_add_json["remote"]["capabilities"]["policy_aware_push"],
        true
    );
    assert!(
        !repo.join(".claw").join("remotes.toml").exists(),
        "remote add --dry-run must not create remotes.toml"
    );

    let added = env.run_ok(
        &repo,
        [
            "remote",
            "--json",
            "add",
            "origin",
            "http://127.0.0.1:50051",
            "--kind",
            "grpc",
            "--token-profile",
            "ci",
        ],
    );
    let added_json = added.stdout_json();
    assert_eq!(added_json["schema_version"], 1);
    assert_eq!(added_json["action"], "remote.add");
    assert_eq!(added_json["dry_run"], false);
    assert_eq!(added_json["saved"], true);

    let listed = env.run_ok(&repo, ["remote", "--json", "list"]);
    let listed_json = listed.stdout_json();
    assert_eq!(listed_json["schema_version"], 1);
    assert_eq!(listed_json["action"], "remote.list");
    assert_eq!(listed_json["remote_count"], 1);
    assert_eq!(listed_json["remotes"][0]["name"], "origin");
    assert_eq!(listed_json["remotes"][0]["url"], "http://127.0.0.1:50051");
    assert_eq!(
        listed_json["remotes"][0]["capabilities"]["transport"],
        "grpc"
    );

    let hosted_preview = env.run_ok(
        &repo,
        [
            "remote",
            "--json",
            "add",
            "hosted",
            "https://claw.example",
            "--kind",
            "clawlab",
            "--repo",
            "acme/widgets",
            "--dry-run",
        ],
    );
    let hosted_preview_json = hosted_preview.stdout_json();
    assert_eq!(hosted_preview_json["schema_version"], 1);
    assert_eq!(hosted_preview_json["action"], "remote.add");
    assert_eq!(hosted_preview_json["remote"]["kind"], "clawlab");
    assert_eq!(
        hosted_preview_json["remote"]["capabilities"]["hosted_http"],
        true
    );
    assert_eq!(
        hosted_preview_json["remote"]["capabilities"]["partial_clone"],
        "requires_remote_capability"
    );
    assert_eq!(
        hosted_preview_json["remote"]["capabilities"]["policy_aware_push"],
        "requires_remote_capability"
    );

    let dry_remove = env.run_ok(&repo, ["remote", "--json", "remove", "origin", "--dry-run"]);
    let dry_remove_json = dry_remove.stdout_json();
    assert_eq!(dry_remove_json["schema_version"], 1);
    assert_eq!(dry_remove_json["action"], "remote.remove");
    assert_eq!(dry_remove_json["dry_run"], true);
    assert_eq!(dry_remove_json["saved"], false);

    let still_listed = env.run_ok(&repo, ["remote", "--json", "list"]);
    assert_eq!(still_listed.stdout_json()["remote_count"], 1);

    let removed = env.run_ok(&repo, ["remote", "--json", "remove", "origin"]);
    let removed_json = removed.stdout_json();
    assert_eq!(removed_json["schema_version"], 1);
    assert_eq!(removed_json["action"], "remote.remove");
    assert_eq!(removed_json["saved"], true);

    let empty = env.run_ok(&repo, ["remote", "--json", "list"]);
    let empty_json = empty.stdout_json();
    assert_eq!(empty_json["schema_version"], 1);
    assert_eq!(empty_json["action"], "remote.list");
    assert_eq!(empty_json["remote_count"], 0);
}

#[test]
fn remote_config_parse_errors_are_reported_without_overwriting_config() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("remote-invalid-config");
    let config_path = repo.join(".claw").join("remotes.toml");
    let broken_config = "[remotes.origin\nurl = \"http://127.0.0.1:50051\"\n";
    env.write_file(&config_path, broken_config);

    let listed = env.run_fail(
        &repo,
        ["--error-format", "json", "remote", "--json", "list"],
    );
    let error = listed.stderr_json();
    assert_eq!(error["schema_version"], 1);
    assert_eq!(error["code"], "CONFIG_ERROR");
    assert_eq!(error["exit_code"], 4);
    assert!(error["message"]
        .as_str()
        .expect("error message")
        .contains("invalid remote config"));
    assert_eq!(env.read_file(&config_path), broken_config);

    let add = env.run_fail(
        &repo,
        [
            "--error-format",
            "json",
            "remote",
            "--json",
            "add",
            "backup",
            "http://127.0.0.1:50052",
        ],
    );
    assert_eq!(add.stderr_json()["code"], "CONFIG_ERROR");
    assert_eq!(
        env.read_file(&config_path),
        broken_config,
        "remote add must not replace an invalid existing remotes.toml"
    );
}

#[test]
fn change_json_receipts_report_schema_actions_and_filters() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("change-json-schema");

    let intent = env.run_ok(
        &repo,
        [
            "intent",
            "create",
            "--title",
            "Stabilize change JSON",
            "--goal",
            "Exercise change receipts",
        ],
    );
    let intent_id = intent.value_after("Created intent: ");

    let created = env.run_ok(
        &repo,
        ["change", "--json", "create", "--intent", intent_id.as_str()],
    );
    let created_json = created.stdout_json();
    assert_eq!(created_json["schema_version"], 1);
    assert_eq!(created_json["action"], "change.create");
    assert_eq!(created_json["created"], true);
    assert_eq!(created_json["change"]["intent_id"], intent_id);
    assert_eq!(created_json["change"]["status"], "open");
    let change_id = created_json["change"]["id"]
        .as_str()
        .expect("created change id")
        .to_string();

    let shown = env.run_ok(&repo, ["change", "--json", "show", change_id.as_str()]);
    let shown_json = shown.stdout_json();
    assert_eq!(shown_json["schema_version"], 1);
    assert_eq!(shown_json["action"], "change.show");
    assert_eq!(shown_json["change"]["id"], change_id);

    let listed = env.run_ok(
        &repo,
        ["change", "--json", "list", "--intent", intent_id.as_str()],
    );
    let listed_json = listed.stdout_json();
    assert_eq!(listed_json["schema_version"], 1);
    assert_eq!(listed_json["action"], "change.list");
    assert_eq!(listed_json["change_count"], 1);
    assert_eq!(listed_json["filters"]["intent"], intent_id);
    assert_eq!(listed_json["changes"][0]["id"], change_id);

    let updated = env.run_ok(
        &repo,
        ["change", "--json", "status", change_id.as_str(), "ready"],
    );
    let updated_json = updated.stdout_json();
    assert_eq!(updated_json["schema_version"], 1);
    assert_eq!(updated_json["action"], "change.status");
    assert_eq!(updated_json["updated"], true);
    assert_eq!(updated_json["change"]["id"], change_id);
    assert_eq!(updated_json["change"]["status"], "ready");
}

#[test]
fn intent_json_receipts_report_schema_actions_and_policy_state() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("intent-json-schema");

    let created = env.run_ok(
        &repo,
        [
            "intent",
            "--json",
            "create",
            "--title",
            "Stabilize intent JSON",
            "--goal",
            "Exercise intent receipts",
        ],
    );
    let created_json = created.stdout_json();
    assert_eq!(created_json["schema_version"], 1);
    assert_eq!(created_json["action"], "intent.create");
    assert_eq!(created_json["created"], true);
    assert_eq!(created_json["intent"]["title"], "Stabilize intent JSON");
    let intent_id = created_json["intent"]["id"]
        .as_str()
        .expect("created intent id")
        .to_string();

    let shown = env.run_ok(&repo, ["intent", "--json", "show", intent_id.as_str()]);
    let shown_json = shown.stdout_json();
    assert_eq!(shown_json["schema_version"], 1);
    assert_eq!(shown_json["action"], "intent.show");
    assert_eq!(shown_json["intent"]["id"], intent_id);

    let listed = env.run_ok(&repo, ["intent", "--json", "list"]);
    let listed_json = listed.stdout_json();
    assert_eq!(listed_json["schema_version"], 1);
    assert_eq!(listed_json["action"], "intent.list");
    assert_eq!(listed_json["intent_count"], 1);
    assert_eq!(listed_json["intents"][0]["id"], intent_id);

    let updated = env.run_ok(
        &repo,
        [
            "intent",
            "--json",
            "update",
            intent_id.as_str(),
            "--status",
            "done",
        ],
    );
    let updated_json = updated.stdout_json();
    assert_eq!(updated_json["schema_version"], 1);
    assert_eq!(updated_json["action"], "intent.update");
    assert_eq!(updated_json["updated"], true);
    assert_eq!(updated_json["intent"]["status"], "done");

    let acceptance = env.run_ok(
        &repo,
        ["intent", "--json", "run-acceptance", intent_id.as_str()],
    );
    let acceptance_json = acceptance.stdout_json();
    assert_eq!(acceptance_json["schema_version"], 1);
    assert_eq!(acceptance_json["action"], "intent.run_acceptance");
    assert_eq!(acceptance_json["passed"], true);
    assert_eq!(acceptance_json["count"], 0);

    let graph = env.run_ok(&repo, ["intent", "--json", "graph"]);
    let graph_json = graph.stdout_json();
    assert_eq!(graph_json["schema_version"], 1);
    assert_eq!(graph_json["action"], "intent.graph");
    assert_eq!(graph_json["intent_count"], 1);
    assert!(graph_json["nodes"]
        .as_array()
        .expect("intent graph nodes")
        .iter()
        .any(|node| node["type"] == "intent" && node["id"] == format!("intent:{intent_id}")));

    env.run_ok(
        &repo,
        ["policy", "create", "--id", "ci-required", "--check", "test"],
    );

    let policy_list = env.run_ok(
        &repo,
        ["intent", "--json", "policy", "list", intent_id.as_str()],
    );
    let policy_list_json = policy_list.stdout_json();
    assert_eq!(policy_list_json["schema_version"], 1);
    assert_eq!(policy_list_json["action"], "intent.policy.list");
    assert_eq!(policy_list_json["intent_id"], intent_id);
    assert_eq!(
        policy_list_json["policy_refs"]
            .as_array()
            .expect("initial policy refs")
            .len(),
        0
    );

    let dry_add = env.run_ok(
        &repo,
        [
            "intent",
            "--json",
            "policy",
            "add",
            intent_id.as_str(),
            "ci-required",
            "--dry-run",
        ],
    );
    let dry_add_json = dry_add.stdout_json();
    assert_eq!(dry_add_json["schema_version"], 1);
    assert_eq!(dry_add_json["action"], "intent.policy.add");
    assert_eq!(dry_add_json["dry_run"], true);
    assert_eq!(dry_add_json["changed"], true);
    assert_eq!(dry_add_json["policy_ref"], "ci-required");
    assert_eq!(dry_add_json["new_object_id"], serde_json::Value::Null);

    let after_dry_add = env.run_ok(
        &repo,
        ["intent", "--json", "policy", "list", intent_id.as_str()],
    );
    assert_eq!(
        after_dry_add.stdout_json()["policy_refs"]
            .as_array()
            .expect("policy refs after dry-run")
            .len(),
        0
    );

    let added = env.run_ok(
        &repo,
        [
            "intent",
            "--json",
            "policy",
            "add",
            intent_id.as_str(),
            "ci-required",
        ],
    );
    let added_json = added.stdout_json();
    assert_eq!(added_json["schema_version"], 1);
    assert_eq!(added_json["action"], "intent.policy.add");
    assert_eq!(added_json["dry_run"], false);
    assert_eq!(added_json["changed"], true);

    let dry_remove = env.run_ok(
        &repo,
        [
            "intent",
            "--json",
            "policy",
            "remove",
            intent_id.as_str(),
            "ci-required",
            "--dry-run",
        ],
    );
    let dry_remove_json = dry_remove.stdout_json();
    assert_eq!(dry_remove_json["schema_version"], 1);
    assert_eq!(dry_remove_json["action"], "intent.policy.remove");
    assert_eq!(dry_remove_json["dry_run"], true);
    assert_eq!(dry_remove_json["changed"], true);

    let removed = env.run_ok(
        &repo,
        [
            "intent",
            "--json",
            "policy",
            "remove",
            intent_id.as_str(),
            "ci-required",
        ],
    );
    let removed_json = removed.stdout_json();
    assert_eq!(removed_json["schema_version"], 1);
    assert_eq!(removed_json["action"], "intent.policy.remove");
    assert_eq!(removed_json["dry_run"], false);
    assert_eq!(removed_json["changed"], true);

    env.run_ok(
        &repo,
        [
            "intent",
            "--json",
            "update",
            intent_id.as_str(),
            "--status",
            "blocked",
        ],
    );
    let blocked_graph = env.run_ok(&repo, ["intent", "--json", "graph"]);
    let blocked_graph_json = blocked_graph.stdout_json();
    assert!(blocked_graph_json["nodes"]
        .as_array()
        .expect("blocked graph nodes")
        .iter()
        .any(|node| node["type"] == "blocker" && node["intent_id"] == intent_id));
    assert!(blocked_graph_json["edges"]
        .as_array()
        .expect("blocked graph edges")
        .iter()
        .any(|edge| edge["relation"] == "blocks" && edge["to"] == format!("intent:{intent_id}")));
}

#[test]
fn agent_lifecycle_json_receipts_report_schema_actions_and_state() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("agent-json-schema");

    let keygen = env.run_ok(
        &repo,
        ["agent", "keygen", "--name", "json-keygen-agent", "--json"],
    );
    let keygen_json = keygen.stdout_json();
    assert_eq!(keygen_json["schema_version"], 1);
    assert_eq!(keygen_json["action"], "agent.keygen");
    assert_eq!(keygen_json["agent_id"], "json-keygen-agent");
    assert!(keygen_json["public_key"].as_str().is_some());

    let registered = env.run_ok(
        &repo,
        ["agent", "register", "--name", "json-agent", "--json"],
    );
    let registered_json = registered.stdout_json();
    assert_eq!(registered_json["schema_version"], 1);
    assert_eq!(registered_json["action"], "agent.register");
    assert_eq!(registered_json["created"], true);
    assert_eq!(registered_json["agent_id"], "json-agent");
    assert_eq!(registered_json["status"], "active");

    let status = env.run_ok(&repo, ["agent", "status", "json-agent", "--json"]);
    let status_json = status.stdout_json();
    assert_eq!(status_json["schema_version"], 1);
    assert_eq!(status_json["action"], "agent.status");
    assert_eq!(status_json["found"], true);
    assert_eq!(status_json["kind"], "registered");
    assert_eq!(status_json["status"], "active");

    let listed = env.run_ok(&repo, ["agent", "list", "--json"]);
    let listed_json = listed.stdout_json();
    assert_eq!(listed_json["schema_version"], 1);
    assert_eq!(listed_json["action"], "agent.list");
    assert_eq!(listed_json["agent_count"], 1);
    assert_eq!(listed_json["agents"][0]["agent_id"], "json-agent");

    let dry_rotate = env.run_ok(
        &repo,
        [
            "agent",
            "rotate",
            "--name",
            "json-agent",
            "--version",
            "next",
            "--dry-run",
            "--json",
        ],
    );
    let dry_rotate_json = dry_rotate.stdout_json();
    assert_eq!(dry_rotate_json["schema_version"], 1);
    assert_eq!(dry_rotate_json["action"], "agent.rotate");
    assert_eq!(dry_rotate_json["dry_run"], true);
    assert_eq!(dry_rotate_json["replacement_version"], "next");

    let dry_quarantine = env.run_ok(
        &repo,
        [
            "agent",
            "quarantine",
            "--name",
            "json-agent",
            "--reason",
            "runner drift",
            "--dry-run",
            "--json",
        ],
    );
    let dry_quarantine_json = dry_quarantine.stdout_json();
    assert_eq!(dry_quarantine_json["schema_version"], 1);
    assert_eq!(dry_quarantine_json["action"], "agent.quarantine");
    assert_eq!(dry_quarantine_json["dry_run"], true);
    assert_eq!(dry_quarantine_json["current_status"], "active");

    let quarantined = env.run_ok(
        &repo,
        [
            "agent",
            "quarantine",
            "--name",
            "json-agent",
            "--reason",
            "runner drift",
            "--json",
        ],
    );
    let quarantined_json = quarantined.stdout_json();
    assert_eq!(quarantined_json["schema_version"], 1);
    assert_eq!(quarantined_json["action"], "agent.quarantine");
    assert_eq!(quarantined_json["dry_run"], false);
    assert_eq!(quarantined_json["changed"], true);
    assert_eq!(quarantined_json["reason"], "runner drift");

    let dry_unquarantine = env.run_ok(
        &repo,
        [
            "agent",
            "unquarantine",
            "--name",
            "json-agent",
            "--dry-run",
            "--json",
        ],
    );
    let dry_unquarantine_json = dry_unquarantine.stdout_json();
    assert_eq!(dry_unquarantine_json["schema_version"], 1);
    assert_eq!(dry_unquarantine_json["action"], "agent.unquarantine");
    assert_eq!(dry_unquarantine_json["dry_run"], true);
    assert_eq!(dry_unquarantine_json["current_status"], "quarantined");

    let unquarantined = env.run_ok(
        &repo,
        ["agent", "unquarantine", "--name", "json-agent", "--json"],
    );
    let unquarantined_json = unquarantined.stdout_json();
    assert_eq!(unquarantined_json["schema_version"], 1);
    assert_eq!(unquarantined_json["action"], "agent.unquarantine");
    assert_eq!(unquarantined_json["dry_run"], false);
    assert_eq!(unquarantined_json["changed"], true);
    assert_eq!(unquarantined_json["status"], "active");

    let dry_revoke = env.run_ok(
        &repo,
        [
            "agent",
            "revoke",
            "--name",
            "json-agent",
            "--reason",
            "compromised",
            "--dry-run",
            "--json",
        ],
    );
    let dry_revoke_json = dry_revoke.stdout_json();
    assert_eq!(dry_revoke_json["schema_version"], 1);
    assert_eq!(dry_revoke_json["action"], "agent.revoke");
    assert_eq!(dry_revoke_json["dry_run"], true);
    assert_eq!(dry_revoke_json["reason"], "compromised");

    let revoked = env.run_ok(
        &repo,
        [
            "agent",
            "revoke",
            "--name",
            "json-agent",
            "--reason",
            "compromised",
            "--json",
        ],
    );
    let revoked_json = revoked.stdout_json();
    assert_eq!(revoked_json["schema_version"], 1);
    assert_eq!(revoked_json["action"], "agent.revoke");
    assert_eq!(revoked_json["dry_run"], false);
    assert_eq!(revoked_json["changed"], true);
    assert_eq!(revoked_json["reason"], "compromised");
}

#[test]
fn migration_wizard_imports_branches_and_infers_team_objects() {
    let env = CliTestEnv::new();
    let repo = env.init_repo("migration-wizard");
    let git_repo = env.repo_path("migration-source-git");
    fs::create_dir_all(&git_repo).expect("create git source repo");
    run_git_ok(&git_repo, &["init", "-q"]);
    run_git_ok(&git_repo, &["config", "user.name", "Claw Tests"]);
    run_git_ok(&git_repo, &["config", "user.email", "tests@example.com"]);
    fs::write(git_repo.join("README.md"), "main\n").expect("write git source file");
    run_git_ok(&git_repo, &["add", "README.md"]);
    run_git_ok(&git_repo, &["commit", "-q", "-m", "initial"]);
    run_git_ok(&git_repo, &["branch", "-M", "main"]);
    run_git_ok(&git_repo, &["checkout", "-q", "-b", "feature/login"]);
    fs::write(git_repo.join("login.txt"), "login\n").expect("write feature file");
    run_git_ok(&git_repo, &["add", "login.txt"]);
    run_git_ok(&git_repo, &["commit", "-q", "-m", "Add login"]);

    fs::write(
        repo.join("provider.json"),
        r#"{
  "pull_requests": [{
    "title": "Add login",
    "body": "Implements the login workflow.",
    "head": {"ref": "feature/login"},
    "html_url": "https://example.test/pulls/7",
    "checks": [{"name": "ci", "status": "completed"}],
    "reviews": [{"user": {"login": "alice"}, "state": "APPROVED", "submitted_at": "2026-05-20T00:00:00Z"}]
  }],
  "branch_protection": {
    "required_status_checks": {"contexts": ["lint"]},
    "required_pull_request_reviews": {
      "required_approving_review_count": 2,
      "require_code_owner_reviews": true,
      "dismiss_stale_reviews": true,
      "require_last_push_approval": true
    }
  }
}
"#,
    )
    .expect("write provider metadata");

    let dry_run = env.run_ok(
        &repo,
        [
            "migration",
            "wizard",
            "--git-dir",
            git_repo
                .join(".git")
                .to_str()
                .expect("git source path utf-8"),
            "--metadata-file",
            "provider.json",
            "--read-notes",
            "--dry-run",
            "--json",
        ],
    );
    let dry_json = dry_run.stdout_json();
    assert_eq!(dry_json["schema_version"], 1);
    assert_eq!(dry_json["action"], "migration.wizard");
    assert_eq!(dry_json["dry_run"], true);
    assert_eq!(dry_json["branch_count"], 2);
    assert_eq!(dry_json["metadata_summary"]["branch_metadata_records"], 1);
    assert_eq!(dry_json["metadata_summary"]["branch_protection_records"], 1);
    assert_eq!(dry_json["metadata_summary"]["matched_by_branch"], 1);
    assert_eq!(dry_json["metadata_summary"]["fallback_branches"], 1);

    let store = ClawStore::open(&repo).expect("open repo after dry-run");
    assert!(store
        .get_ref("heads/migrated/feature/login")
        .expect("read dry-run migrated ref")
        .is_none());

    let migrated = env.run_ok(
        &repo,
        [
            "migration",
            "wizard",
            "--git-dir",
            git_repo
                .join(".git")
                .to_str()
                .expect("git source path utf-8"),
            "--metadata-file",
            "provider.json",
            "--read-notes",
            "--json",
        ],
    );
    let migrated_json = migrated.stdout_json();
    assert_eq!(migrated_json["schema_version"], 1);
    assert_eq!(migrated_json["action"], "migration.wizard");
    assert_eq!(migrated_json["dry_run"], false);
    assert_eq!(migrated_json["branch_count"], 2);
    assert_eq!(migrated_json["notes_imported"], 0);
    assert_eq!(migrated_json["metadata_summary"]["required_check_count"], 2);
    assert_eq!(
        migrated_json["metadata_summary"]["required_reviewer_count"],
        1
    );
    assert_eq!(
        migrated_json["metadata_summary"]["required_approving_review_count"],
        2
    );
    assert_eq!(
        migrated_json["metadata_summary"]["require_code_owner_reviews"],
        true
    );
    assert_eq!(
        migrated_json["metadata_summary"]["dismiss_stale_reviews"],
        true
    );
    assert_eq!(
        migrated_json["metadata_summary"]["require_last_push_approval"],
        true
    );
    assert_eq!(
        migrated_json["suggested_policy"]["required_checks"]
            .as_array()
            .expect("required checks")
            .iter()
            .map(|value| value.as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["ci", "lint"]
    );
    assert_eq!(
        migrated_json["suggested_policy"]["review_requirements"]["required_approving_review_count"],
        2
    );
    assert_eq!(
        migrated_json["suggested_policy"]["review_requirements"]["require_code_owner_reviews"],
        true
    );
    assert!(migrated_json["suggested_policy"]["migration_warnings"]
        .as_array()
        .expect("migration warnings")
        .iter()
        .any(|warning| warning
            .as_str()
            .unwrap_or_default()
            .contains("code owner reviews")));

    let feature = migrated_json["branches"]
        .as_array()
        .expect("branches array")
        .iter()
        .find(|branch| branch["git_ref"] == "refs/heads/feature/login")
        .expect("feature branch report");
    assert_eq!(feature["title"], "Add login");
    assert_eq!(feature["metadata_match"], "branch");
    assert_eq!(feature["metadata_links"][0], "https://example.test/pulls/7");
    let main = migrated_json["branches"]
        .as_array()
        .expect("branches array")
        .iter()
        .find(|branch| branch["git_ref"] == "refs/heads/main")
        .expect("main branch report");
    assert_eq!(main["metadata_match"], "fallback");

    let store = ClawStore::open(&repo).expect("open repo after migration");
    let feature_revision = store
        .get_ref("heads/migrated/feature/login")
        .expect("read feature migrated ref")
        .expect("feature migrated ref exists");
    assert!(store
        .get_ref("heads/migrated/main")
        .expect("read main migrated ref")
        .is_some());
    assert!(store
        .get_ref("policies/migration-suggested")
        .expect("read suggested policy ref")
        .is_some());
    let metadata_ref = migrated_json["metadata_ref"]
        .as_str()
        .expect("metadata ref string");
    assert!(store
        .get_ref(metadata_ref)
        .expect("read metadata ref")
        .is_some());

    let intent_ref = format!(
        "intents/{}",
        feature["intent_id"].as_str().expect("intent id string")
    );
    let intent_obj = store
        .load_object(
            &store
                .get_ref(&intent_ref)
                .expect("read inferred intent ref")
                .expect("inferred intent ref exists"),
        )
        .expect("load inferred intent");
    let Object::Intent(intent) = intent_obj else {
        panic!("inferred intent ref must point to an intent");
    };
    assert_eq!(intent.title, "Add login");
    assert!(intent
        .links
        .contains(&"https://example.test/pulls/7".to_string()));
    assert!(intent
        .acceptance_tests
        .contains(&"evidence:ci=pass".to_string()));

    let change_ref = format!(
        "changes/{}",
        feature["change_id"].as_str().expect("change id string")
    );
    let change_obj = store
        .load_object(
            &store
                .get_ref(&change_ref)
                .expect("read inferred change ref")
                .expect("inferred change ref exists"),
        )
        .expect("load inferred change");
    let Object::Change(change) = change_obj else {
        panic!("inferred change ref must point to a change");
    };
    assert_eq!(change.head_revision, Some(feature_revision));
}

#[tokio::test]
async fn remote_compatibility_classification_is_exercised_over_hello() {
    let local = env!("CARGO_PKG_VERSION");
    let cases = [
        (local.to_string(), CompatibilityLevel::Full),
        (adjacent_minor_version(local), CompatibilityLevel::Limited),
        (next_major_version(local), CompatibilityLevel::Unsupported),
    ];

    for (server_version, expected) in cases {
        let (endpoint, shutdown) = spawn_hello_remote(server_version.clone()).await;
        let mut client = connect_with_retry(&endpoint).await;
        let hello = client.hello().await.expect("remote hello");
        let report = compatibility_report(local, &hello.server_version);

        assert_eq!(hello.server_version, server_version);
        assert_eq!(report.level, expected);
        assert!(
            hello.capabilities.iter().any(|cap| cap == "partial-clone"),
            "compatibility remotes should expose the baseline sync capability"
        );

        let _ = shutdown.send(());
    }
}

fn seed_policy_evidence_revision(repo: &Path) -> String {
    let store = ClawStore::open(repo).expect("open seeded store");
    let blob_id = store
        .store_object(&Object::Blob(Blob {
            data: b"tracked through git notes\n".to_vec(),
            media_type: Some("text/plain".to_string()),
        }))
        .expect("store blob");
    let tree_id = store
        .store_object(&Object::Tree(Tree {
            entries: vec![TreeEntry {
                name: "provenance.txt".to_string(),
                mode: FileMode::Regular,
                object_id: blob_id,
            }],
        }))
        .expect("store tree");
    let revision_id = store
        .store_object(&Object::Revision(Revision {
            change_id: None,
            parents: vec![],
            patches: vec![],
            snapshot_base: None,
            tree: Some(tree_id),
            capsule_id: None,
            author: "integration-test".to_string(),
            created_at_ms: 1_700_000_000_000,
            summary: "Revision with policy evidence".to_string(),
            policy_evidence: vec![
                "ci/git-notes=pass".to_string(),
                "artifact/provenance=present".to_string(),
            ],
        }))
        .expect("store revision");
    store
        .set_ref("heads/main", &revision_id)
        .expect("seed main ref");
    fs::write(repo.join("provenance.txt"), "tracked through git notes\n")
        .expect("materialize provenance file");
    revision_id.to_hex()
}

fn run_git_ok(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("run git {:?}: {err}", args));
    assert!(
        output.status.success(),
        "git command failed in {}\n$ git {}\nstdout:\n{}\nstderr:\n{}",
        cwd.display(),
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[derive(Clone)]
struct HelloOnlyService {
    server_version: String,
}

#[tonic::async_trait]
impl SyncService for HelloOnlyService {
    type FetchObjectsStream = ReceiverStream<Result<ObjectChunk, Status>>;

    async fn hello(
        &self,
        _request: Request<HelloRequest>,
    ) -> Result<Response<HelloResponse>, Status> {
        Ok(Response::new(HelloResponse {
            server_version: self.server_version.clone(),
            capabilities: vec!["partial-clone".to_string()],
        }))
    }

    async fn advertise_refs(
        &self,
        _request: Request<AdvertiseRefsRequest>,
    ) -> Result<Response<AdvertiseRefsResponse>, Status> {
        Err(Status::unimplemented("hello-only compatibility service"))
    }

    async fn fetch_objects(
        &self,
        _request: Request<FetchObjectsRequest>,
    ) -> Result<Response<Self::FetchObjectsStream>, Status> {
        Err(Status::unimplemented("hello-only compatibility service"))
    }

    async fn push_objects(
        &self,
        _request: Request<tonic::Streaming<ObjectChunk>>,
    ) -> Result<Response<PushObjectsResponse>, Status> {
        Err(Status::unimplemented("hello-only compatibility service"))
    }

    async fn update_refs(
        &self,
        _request: Request<UpdateRefsRequest>,
    ) -> Result<Response<UpdateRefsResponse>, Status> {
        Err(Status::unimplemented("hello-only compatibility service"))
    }
}

async fn spawn_hello_remote(server_version: String) -> (String, oneshot::Sender<()>) {
    let addr = free_local_addr();
    let endpoint = format!("http://{addr}");
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let service = HelloOnlyService { server_version };

    tokio::spawn(async move {
        Server::builder()
            .add_service(SyncServiceServer::new(service))
            .serve_with_shutdown(addr, async {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("serve compatibility hello remote");
    });

    (endpoint, shutdown_tx)
}

async fn connect_with_retry(endpoint: &str) -> SyncClient {
    let mut last_error = String::new();
    for _ in 0..50 {
        match SyncClient::connect(endpoint).await {
            Ok(client) => return client,
            Err(err) => {
                last_error = err.to_string();
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }
    }

    panic!("failed to connect to compatibility remote {endpoint}: {last_error}");
}

fn free_local_addr() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local test port");
    let addr = listener.local_addr().expect("read local test port");
    drop(listener);
    addr
}

fn adjacent_minor_version(version: &str) -> String {
    let (major, minor) = major_minor(version);
    format!("{major}.{}.0", minor + 1)
}

fn next_major_version(version: &str) -> String {
    let (major, _minor) = major_minor(version);
    format!("{}.0.0", major + 1)
}

fn major_minor(version: &str) -> (u64, u64) {
    let clean = version.trim_start_matches('v');
    let mut parts = clean.split('.');
    let major = parts
        .next()
        .and_then(|part| part.parse::<u64>().ok())
        .unwrap_or(0);
    let minor = parts
        .next()
        .and_then(|part| part.parse::<u64>().ok())
        .unwrap_or(0);
    (major, minor)
}
