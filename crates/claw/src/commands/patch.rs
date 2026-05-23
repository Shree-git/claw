use clap::{Args, Subcommand};
use std::path::PathBuf;

use claw_core::object::Object;
use claw_core::types::{Blob, Patch, PatchOp};
use claw_patch::CodecRegistry;
use claw_store::ClawStore;

use crate::config::find_repo_root;

use super::object_refs::resolve_object_ref_or_id;

#[derive(Args)]
pub struct PatchArgs {
    /// Output command results as JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: PatchCommand,
}

#[derive(Subcommand)]
enum PatchCommand {
    /// Create a patch from two files
    Create {
        /// Old file
        #[arg(long)]
        old: PathBuf,
        /// New file
        #[arg(long)]
        new: PathBuf,
        /// Target path in repo
        #[arg(short, long)]
        path: String,
    },
    /// Apply a patch to a file
    Apply {
        /// Patch object ID
        #[arg(short, long)]
        patch: String,
        /// File to apply to
        #[arg(short, long)]
        file: PathBuf,
    },
    /// Show a patch
    Show {
        /// Patch object ID
        id: String,
    },
    /// Explain whether two patches commute and show the reordered operations
    Commute {
        /// Left patch object ID
        #[arg(long)]
        left: String,
        /// Right patch object ID
        #[arg(long)]
        right: String,
    },
    /// Invert a patch and show the undo operations
    Invert {
        /// Patch object ID
        #[arg(short, long)]
        patch: String,
    },
    /// Three-way merge files with the semantic codec for a repository path
    Merge3 {
        /// Common base file
        #[arg(long)]
        base: PathBuf,
        /// Left edited file
        #[arg(long)]
        left: PathBuf,
        /// Right edited file
        #[arg(long)]
        right: PathBuf,
        /// Target path in repo used to select the codec
        #[arg(short, long)]
        path: String,
        /// Write merged output to this file instead of stdout
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Explain commute, conflict, inversion, and reorder behavior for two patches
    Workbench {
        /// Left patch object ID
        #[arg(long)]
        left: String,
        /// Right patch object ID
        #[arg(long)]
        right: String,
    },
    /// List registered codecs and optionally resolve the codec for a path
    Codecs {
        /// Repository path to resolve through extension and semantic path matchers
        #[arg(long)]
        path: Option<String>,
    },
}

pub fn run(args: PatchArgs) -> anyhow::Result<()> {
    match args.command {
        PatchCommand::Create { old, new, path } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let registry = CodecRegistry::default();

            let old_data = std::fs::read(&old)?;
            let new_data = std::fs::read(&new)?;

            // Store blobs
            let old_blob = Object::Blob(Blob {
                data: old_data.clone(),
                media_type: None,
            });
            let new_blob = Object::Blob(Blob {
                data: new_data.clone(),
                media_type: None,
            });
            let old_id = store.store_object(&old_blob)?;
            let new_id = store.store_object(&new_blob)?;

            let codec = codec_for_path(&registry, &path)?;

            let ops = codec.diff(&old_data, &new_data)?;

            let patch = Patch {
                target_path: path.clone(),
                codec_id: codec.id().to_string(),
                base_object: Some(old_id),
                result_object: Some(new_id),
                ops,
                codec_payload: None,
            };

            let patch_id = store.store_object(&Object::Patch(patch.clone()))?;
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "patch.create",
                        "created": true,
                        "patch": patch_id.to_string(),
                        "patch_hex": patch_id.to_hex(),
                        "path": path,
                        "codec": codec.id(),
                        "ops": patch.ops,
                    }))?
                );
            } else {
                println!("Created patch: {patch_id}");
                println!("  Path: {path}");
                println!("  Codec: {}", codec.id());
            }
        }
        PatchCommand::Apply { patch, file } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let registry = CodecRegistry::default();

            let (patch_id, p) = load_patch(&store, &patch)?;

            let base_data = std::fs::read(&file)?;
            let codec = registry.get(&p.codec_id)?;
            let result = codec.apply(&base_data, &p.ops)?;
            let bytes_written = result.len();

            write_file_atomic(&file, &result)?;
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "patch.apply",
                        "applied": true,
                        "patch": patch_summary(patch_id, &p),
                        "file": file.display().to_string(),
                        "codec": p.codec_id,
                        "bytes_written": bytes_written,
                    }))?
                );
            } else {
                println!("Applied patch to {}", file.display());
            }
        }
        PatchCommand::Show { id } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;

            let (patch_id, p) = load_patch(&store, &id)?;
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "patch.show",
                        "patch": patch_id.to_string(),
                        "patch_hex": patch_id.to_hex(),
                        "target_path": p.target_path,
                        "codec": p.codec_id,
                        "base_object": p.base_object.map(|id| id.to_string()),
                        "result_object": p.result_object.map(|id| id.to_string()),
                        "ops": p.ops,
                    }))?
                );
            } else {
                println!("Patch: {id}");
                println!("  Target: {}", p.target_path);
                println!("  Codec: {}", p.codec_id);
                println!("  Ops: {}", p.ops.len());
                for (i, op) in p.ops.iter().enumerate() {
                    println!("    [{i}] {} at {}", op.op_type, op.address);
                }
            }
        }
        PatchCommand::Commute { left, right } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let registry = CodecRegistry::default();
            let (left_id, left_patch) = load_patch(&store, &left)?;
            let (right_id, right_patch) = load_patch(&store, &right)?;

            if left_patch.codec_id != right_patch.codec_id {
                let reason = format!(
                    "codec mismatch: left={} right={}",
                    left_patch.codec_id, right_patch.codec_id
                );
                print_commute_result(args.json, &left_patch, &right_patch, false, &reason, None)?;
                return Ok(());
            }
            if left_patch.target_path != right_patch.target_path {
                let reason = format!(
                    "target path mismatch: left={} right={}",
                    left_patch.target_path, right_patch.target_path
                );
                print_commute_result(args.json, &left_patch, &right_patch, false, &reason, None)?;
                return Ok(());
            }

            let codec = registry.get(&left_patch.codec_id)?;
            match codec.commute(&left_patch.ops, &right_patch.ops) {
                Ok((right_then_left_right, right_then_left_left)) => {
                    if args.json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "schema_version": 1,
                                "action": "patch.commute",
                                "commutes": true,
                                "reason": "codec accepted operation reorder",
                                "left": patch_summary(left_id, &left_patch),
                                "right": patch_summary(right_id, &right_patch),
                                "reordered": {
                                    "right_after_left": right_then_left_right,
                                    "left_after_right": right_then_left_left,
                                },
                            }))?
                        );
                    } else {
                        println!("Patches commute.");
                        println!("  Target: {}", left_patch.target_path);
                        println!("  Codec: {}", left_patch.codec_id);
                        println!(
                            "  Reordered ops: right_after_left={} left_after_right={}",
                            right_then_left_right.len(),
                            right_then_left_left.len()
                        );
                    }
                }
                Err(err) => {
                    let reason = err.to_string();
                    print_commute_result(
                        args.json,
                        &left_patch,
                        &right_patch,
                        false,
                        &reason,
                        None,
                    )?;
                }
            }
        }
        PatchCommand::Invert { patch } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let registry = CodecRegistry::default();
            let (patch_id, patch) = load_patch(&store, &patch)?;
            let codec = registry.get(&patch.codec_id)?;
            let inverted = codec.invert(&patch.ops)?;
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "patch.invert",
                        "patch": patch_summary(patch_id, &patch),
                        "invertible": true,
                        "ops": inverted,
                    }))?
                );
            } else {
                println!("Patch is invertible.");
                println!("  Patch: {}", patch_id.to_hex());
                println!("  Inverted ops: {}", inverted.len());
                for (i, op) in inverted.iter().enumerate() {
                    println!("    [{i}] {} at {}", op.op_type, op.address);
                }
            }
        }
        PatchCommand::Merge3 {
            base,
            left,
            right,
            path,
            out,
        } => {
            let registry = CodecRegistry::default();
            let codec = codec_for_path(&registry, &path)?;
            let base_data = std::fs::read(&base)?;
            let left_data = std::fs::read(&left)?;
            let right_data = std::fs::read(&right)?;
            let merged = codec.merge3(&base_data, &left_data, &right_data)?;

            if let Some(out) = out {
                write_file_atomic(&out, &merged)?;
                if args.json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "schema_version": 1,
                            "action": "patch.merge3",
                            "merged": true,
                            "path": path,
                            "codec": codec.id(),
                            "out": out,
                            "bytes": merged.len(),
                        }))?
                    );
                } else {
                    println!("Merged {} with {}", path, codec.id());
                    println!("  Output: {}", out.display());
                }
            } else if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "patch.merge3",
                        "merged": true,
                        "path": path,
                        "codec": codec.id(),
                        "bytes": merged.len(),
                        "data_utf8": String::from_utf8_lossy(&merged),
                    }))?
                );
            } else {
                print!("{}", String::from_utf8_lossy(&merged));
            }
        }
        PatchCommand::Workbench { left, right } => {
            let root = find_repo_root()?;
            let store = ClawStore::open(&root)?;
            let registry = CodecRegistry::default();
            let (left_id, left_patch) = load_patch(&store, &left)?;
            let (right_id, right_patch) = load_patch(&store, &right)?;
            let report =
                patch_workbench_report(&registry, left_id, &left_patch, right_id, &right_patch);
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_workbench_report(&report);
            }
        }
        PatchCommand::Codecs { path } => {
            let registry = CodecRegistry::default();
            let codecs = registry.inventory();
            let resolved = path.as_deref().and_then(|path| {
                registry.get_for_path(path).map(|codec| {
                    serde_json::json!({
                        "path": path,
                        "codec": codec.id(),
                    })
                })
            });
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "action": "patch.codecs",
                        "codec_count": codecs.len(),
                        "codecs": codecs,
                        "resolved": resolved,
                    }))?
                );
            } else {
                println!("Patch codecs");
                for codec in &codecs {
                    let extensions = if codec.extensions.is_empty() {
                        "-".to_string()
                    } else {
                        codec
                            .extensions
                            .iter()
                            .map(|ext| format!(".{ext}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    let fallback = if codec.fallback { " fallback" } else { "" };
                    println!(
                        "  {} [{}{}] extensions={}",
                        codec.id, codec.family, fallback, extensions
                    );
                    if !codec.path_matchers.is_empty() {
                        println!("    path matchers: {}", codec.path_matchers.join(", "));
                    }
                }
                if let Some(resolved) = resolved {
                    println!(
                        "Resolved {} -> {}",
                        resolved["path"].as_str().unwrap_or(""),
                        resolved["codec"].as_str().unwrap_or("")
                    );
                } else if let Some(path) = path {
                    println!("Resolved {path} -> none");
                }
            }
        }
    }
    Ok(())
}

fn load_patch(store: &ClawStore, value: &str) -> anyhow::Result<(claw_core::id::ObjectId, Patch)> {
    let patch_id = resolve_object_ref_or_id(store, value)?;
    match store.load_object(&patch_id)? {
        Object::Patch(patch) => Ok((patch_id, patch)),
        _ => anyhow::bail!("object is not a patch: {value}"),
    }
}

fn write_file_atomic(path: &std::path::Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("path has no parent: {}", path.display()))?;
    let parent = if parent.as_os_str().is_empty() {
        std::path::Path::new(".")
    } else {
        parent
    };
    std::fs::create_dir_all(parent)?;

    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    {
        use std::io::Write;

        let file = temp.as_file_mut();
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    temp.persist(path).map_err(|err| err.error)?;
    if let Ok(parent_dir) = std::fs::File::open(parent) {
        parent_dir.sync_all()?;
    }
    Ok(())
}

fn codec_for_path<'a>(
    registry: &'a CodecRegistry,
    path: &str,
) -> anyhow::Result<&'a std::sync::Arc<dyn claw_patch::Codec>> {
    registry
        .get_for_path(path)
        .ok_or_else(|| anyhow::anyhow!("no codec for path: {path}"))
}

fn patch_summary(id: claw_core::id::ObjectId, patch: &Patch) -> serde_json::Value {
    serde_json::json!({
        "id": id.to_string(),
        "hex": id.to_hex(),
        "target_path": patch.target_path,
        "codec": patch.codec_id,
        "ops": patch.ops.len(),
    })
}

fn print_commute_result(
    json: bool,
    left: &Patch,
    right: &Patch,
    commutes: bool,
    reason: &str,
    reordered: Option<(Vec<PatchOp>, Vec<PatchOp>)>,
) -> anyhow::Result<()> {
    if json {
        let reordered = reordered.map(|(right_after_left, left_after_right)| {
            serde_json::json!({
                "right_after_left": right_after_left,
                "left_after_right": left_after_right,
            })
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "action": "patch.commute",
                "commutes": commutes,
                "reason": reason,
                "left": {
                    "target_path": left.target_path,
                    "codec": left.codec_id,
                    "ops": left.ops.len(),
                },
                "right": {
                    "target_path": right.target_path,
                    "codec": right.codec_id,
                    "ops": right.ops.len(),
                },
                "reordered": reordered,
            }))?
        );
    } else if commutes {
        println!("Patches commute.");
        println!("  Reason: {reason}");
    } else {
        println!("Patches do not commute.");
        println!("  Reason: {reason}");
    }
    Ok(())
}

fn patch_workbench_report(
    registry: &CodecRegistry,
    left_id: claw_core::id::ObjectId,
    left: &Patch,
    right_id: claw_core::id::ObjectId,
    right: &Patch,
) -> serde_json::Value {
    let mut blockers = Vec::new();
    if left.codec_id != right.codec_id {
        blockers.push(format!(
            "codec mismatch: left={} right={}",
            left.codec_id, right.codec_id
        ));
    }
    if left.target_path != right.target_path {
        blockers.push(format!(
            "target path mismatch: left={} right={}",
            left.target_path, right.target_path
        ));
    }

    let left_inverse = invert_patch_ops(registry, left);
    let right_inverse = invert_patch_ops(registry, right);
    let (commutes, reason, reordered) = if blockers.is_empty() {
        match registry.get(&left.codec_id) {
            Ok(codec) => match codec.commute(&left.ops, &right.ops) {
                Ok((right_after_left, left_after_right)) => (
                    true,
                    "codec accepted operation reorder".to_string(),
                    Some(serde_json::json!({
                        "right_after_left": right_after_left,
                        "left_after_right": left_after_right,
                    })),
                ),
                Err(err) => (false, err.to_string(), None),
            },
            Err(err) => (false, err.to_string(), None),
        }
    } else {
        (false, blockers.join("; "), None)
    };

    let classification = if commutes {
        "commutes"
    } else if blockers.is_empty() {
        "conflicts"
    } else {
        "not_comparable"
    };
    let analysis = workbench_analysis(
        left,
        right,
        &classification,
        reordered.is_some(),
        &left_inverse,
        &right_inverse,
    );

    serde_json::json!({
        "schema_version": 1,
        "action": "patch.workbench",
        "workbench": true,
        "classification": classification,
        "target_path": if left.target_path == right.target_path { Some(left.target_path.clone()) } else { None },
        "codec": if left.codec_id == right.codec_id { Some(left.codec_id.clone()) } else { None },
        "commute": {
            "commutes": commutes,
            "reason": reason,
            "reordered": reordered,
        },
        "invert": {
            "left": left_inverse,
            "right": right_inverse,
        },
        "patches": {
            "left": patch_summary(left_id, left),
            "right": patch_summary(right_id, right),
        },
        "analysis": analysis,
        "why": workbench_reasons(commutes, blockers, &left_inverse, &right_inverse),
    })
}

fn workbench_analysis(
    left: &Patch,
    right: &Patch,
    classification: &str,
    reorder_available: bool,
    left_inverse: &serde_json::Value,
    right_inverse: &serde_json::Value,
) -> serde_json::Value {
    let left_addresses = op_addresses(&left.ops);
    let right_addresses = op_addresses(&right.ops);
    let overlapping_addresses = overlapping_addresses(&left_addresses, &right_addresses);
    let overlap_count = overlapping_addresses.len();
    let address_relation = if left_addresses.is_empty() && right_addresses.is_empty() {
        "unknown"
    } else if overlap_count == 0 {
        "disjoint"
    } else {
        "overlap"
    };
    let decision = match classification {
        "commutes" => "safe_to_reorder",
        "conflicts" => "conflicting_ops",
        _ => "not_comparable",
    };
    let left_invertible = left_inverse["invertible"].as_bool().unwrap_or(false);
    let right_invertible = right_inverse["invertible"].as_bool().unwrap_or(false);

    serde_json::json!({
        "same_target_path": left.target_path == right.target_path,
        "same_codec": left.codec_id == right.codec_id,
        "same_base_object": left.base_object == right.base_object,
        "same_result_object": left.result_object == right.result_object,
        "left_op_count": left.ops.len(),
        "right_op_count": right.ops.len(),
        "left_addresses": left_addresses,
        "right_addresses": right_addresses,
        "overlapping_addresses": overlapping_addresses,
        "overlap_count": overlap_count,
        "address_relation": address_relation,
        "reorder_available": reorder_available,
        "left_invertible": left_invertible,
        "right_invertible": right_invertible,
        "left_inverse_op_count": left_inverse["op_count"].as_u64(),
        "right_inverse_op_count": right_inverse["op_count"].as_u64(),
        "decision": decision,
    })
}

fn op_addresses(ops: &[PatchOp]) -> Vec<String> {
    let mut addresses = ops.iter().map(|op| op.address.clone()).collect::<Vec<_>>();
    addresses.sort();
    addresses.dedup();
    addresses
}

fn overlapping_addresses(left: &[String], right: &[String]) -> Vec<String> {
    let right = right.iter().collect::<std::collections::BTreeSet<_>>();
    left.iter()
        .filter(|address| right.contains(address))
        .cloned()
        .collect()
}

fn invert_patch_ops(registry: &CodecRegistry, patch: &Patch) -> serde_json::Value {
    match registry
        .get(&patch.codec_id)
        .and_then(|codec| codec.invert(&patch.ops))
    {
        Ok(ops) => serde_json::json!({
            "invertible": true,
            "op_count": ops.len(),
            "ops": ops,
        }),
        Err(err) => serde_json::json!({
            "invertible": false,
            "reason": err.to_string(),
        }),
    }
}

fn workbench_reasons(
    commutes: bool,
    blockers: Vec<String>,
    left_inverse: &serde_json::Value,
    right_inverse: &serde_json::Value,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if blockers.is_empty() {
        if commutes {
            reasons.push("both patches target the same path and codec, and the codec produced a valid reorder".to_string());
        } else {
            reasons.push(
                "both patches target the same path and codec, but the codec rejected the reorder"
                    .to_string(),
            );
        }
    } else {
        reasons.extend(blockers);
    }
    for (side, inverse) in [("left", left_inverse), ("right", right_inverse)] {
        if inverse["invertible"].as_bool().unwrap_or(false) {
            reasons.push(format!("{side} patch has codec-provided undo operations"));
        } else if let Some(reason) = inverse["reason"].as_str() {
            reasons.push(format!("{side} patch is not invertible: {reason}"));
        }
    }
    reasons
}

fn print_workbench_report(report: &serde_json::Value) {
    println!(
        "Patch workbench: {}",
        report["classification"].as_str().unwrap_or("unknown")
    );
    if let Some(path) = report["target_path"].as_str() {
        println!("  Target: {path}");
    }
    if let Some(codec) = report["codec"].as_str() {
        println!("  Codec: {codec}");
    }
    println!(
        "  Commutes: {}",
        report["commute"]["commutes"].as_bool().unwrap_or(false)
    );
    if let Some(reason) = report["commute"]["reason"].as_str() {
        println!("  Reason: {reason}");
    }
    println!(
        "  Invertible: left={} right={}",
        report["invert"]["left"]["invertible"]
            .as_bool()
            .unwrap_or(false),
        report["invert"]["right"]["invertible"]
            .as_bool()
            .unwrap_or(false)
    );
    println!(
        "  Addresses: left={} right={} overlap={}",
        report["analysis"]["left_op_count"].as_u64().unwrap_or(0),
        report["analysis"]["right_op_count"].as_u64().unwrap_or(0),
        report["analysis"]["overlap_count"].as_u64().unwrap_or(0)
    );
    if let Some(decision) = report["analysis"]["decision"].as_str() {
        println!("  Decision: {decision}");
    }
    for reason in report["why"].as_array().into_iter().flatten() {
        if let Some(reason) = reason.as_str() {
            println!("  - {reason}");
        }
    }
}

#[cfg(test)]
mod tests {
    use claw_patch::CodecRegistry;

    use super::codec_for_path;

    #[test]
    fn patch_create_uses_path_aware_codec_registry() {
        let registry = CodecRegistry::default();
        assert_eq!(
            codec_for_path(&registry, "deploy/k8s/service.yaml")
                .unwrap()
                .id(),
            "kubernetes/tree"
        );
        assert_eq!(
            codec_for_path(&registry, "api/openapi.yaml").unwrap().id(),
            "openapi/tree"
        );
        assert_eq!(
            codec_for_path(&registry, "Claw.toml").unwrap().id(),
            "toml/tree"
        );
    }

    #[test]
    fn parses_patch_codecs_path_resolution() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(flatten)]
            args: super::PatchArgs,
        }

        let cli = TestCli::parse_from(["claw", "codecs", "--path", "api/openapi.yaml"]);
        match cli.args.command {
            super::PatchCommand::Codecs { path } => {
                assert_eq!(path.as_deref(), Some("api/openapi.yaml"));
            }
            _ => panic!("expected patch codecs command"),
        }
    }

    #[test]
    fn parses_patch_workbench() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(flatten)]
            args: super::PatchArgs,
        }

        let cli = TestCli::parse_from(["claw", "workbench", "--left", "a", "--right", "b"]);
        match cli.args.command {
            super::PatchCommand::Workbench { left, right } => {
                assert_eq!(left, "a");
                assert_eq!(right, "b");
            }
            _ => panic!("expected patch workbench command"),
        }
    }
}
