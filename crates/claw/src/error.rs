use serde::Serialize;

use claw_store::StoreError;

pub mod exit_codes {
    #[allow(dead_code)]
    pub const OK: i32 = 0;
    pub const GENERAL: i32 = 1;
    #[allow(dead_code)]
    pub const USAGE: i32 = 2;
    pub const NOT_REPOSITORY: i32 = 3;
    pub const CONFIG: i32 = 4;
    pub const IO: i32 = 5;
    pub const AUTH: i32 = 6;
    pub const REMOTE: i32 = 7;
    pub const CONFLICT: i32 = 8;
    pub const WORKTREE_DIRTY: i32 = 9;
    pub const POLICY: i32 = 10;
    pub const COMPATIBILITY: i32 = 11;
}

#[derive(Debug, Clone, Serialize)]
pub struct CliDiagnostic {
    pub schema_version: u8,
    pub code: &'static str,
    pub message: String,
    pub reason: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<&'static str>,
    pub exit_code: i32,
    pub details: serde_json::Value,
}

impl CliDiagnostic {
    pub fn from_usage_error(message: String, kind: clap::error::ErrorKind) -> Self {
        Self {
            schema_version: 1,
            code: "USAGE_ERROR",
            message,
            reason: "The command line did not match a supported Claw command or option shape.",
            remediation: Some("Run `claw --help` or `claw <command> --help` for usage."),
            exit_code: exit_codes::USAGE,
            details: serde_json::json!({
                "kind": format!("{kind:?}"),
            }),
        }
    }

    pub fn from_error(err: &anyhow::Error) -> Self {
        let message = err.to_string();
        let chain = err
            .chain()
            .map(|cause| cause.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let text = chain.to_lowercase();

        let is_not_repository = err
            .chain()
            .any(|cause| cause.is::<crate::config::NotRepositoryError>());

        let mut details = serde_json::Value::Null;
        let store_error = err
            .chain()
            .find_map(|cause| cause.downcast_ref::<StoreError>());

        let (code, exit_code, reason, remediation) = if let Some(StoreError::RefNameCollision {
            requested,
            existing,
        }) = store_error
        {
            details = serde_json::json!({
                "requested": requested,
                "existing": existing,
            });
            (
                "REF_NAME_COLLISION",
                exit_codes::GENERAL,
                "The ref name collides with an existing ref on case-insensitive filesystems.",
                Some(
                    "Choose a ref or branch name that differs by more than letter case, then retry.",
                ),
            )
        } else if let Some(StoreError::InvalidRefName(ref_name)) = store_error {
            details = serde_json::json!({
                "ref_name": ref_name,
            });
            (
                "INVALID_REF_NAME",
                exit_codes::GENERAL,
                "The ref name is not portable or would escape Claw's ref storage directory.",
                Some("Use a relative ref such as `heads/main` without `.`/`..`, backslashes, reserved Windows names, trailing spaces/dots, or path metacharacters."),
            )
        } else if is_not_repository
            || text.contains("not in a claw repository")
            || text.contains("not a claw repository")
            || text.contains("no .claw directory found")
        {
            (
                "NOT_REPOSITORY",
                exit_codes::NOT_REPOSITORY,
                "Claw could not find a `.claw` directory in this path or any parent path.",
                Some(
                    "Run `claw init` in this directory, or `cd` into an existing Claw repository.",
                ),
            )
        } else if text.contains("config") {
            (
                    "CONFIG_ERROR",
                    exit_codes::CONFIG,
                    "Repository configuration could not be loaded or is not compatible with this CLI.",
                    Some("Run `claw doctor` to inspect repository configuration, then fix the reported file."),
                )
        } else if text.contains("no token found")
            || text.contains("missing bearer token")
            || text.contains("invalid bearer token")
            || text.contains("unauthenticated")
            || text.contains("authorization code")
            || text.contains("token exchange failed")
        {
            (
                    "AUTH_ERROR",
                    exit_codes::AUTH,
                    "The remote operation needs an authentication token, but the current profile could not provide one.",
                    Some("Run `claw auth login --profile default`, or import a token with `claw auth token set --stdin`."),
                )
        } else if text.contains("policy") {
            (
                "POLICY_DENIED",
                exit_codes::POLICY,
                "A referenced policy rejected the revision, capsule, evidence, or signer set.",
                Some("Review the policy requirements, add the required evidence/signatures, or run `claw policy eval --json` for details."),
            )
        } else if text.contains("compatibility check failed")
            || text.contains("protocol negotiation failed")
            || text.contains("protocol mismatch")
            || text.contains("unsupported protocol")
            || text.contains("incompatible")
        {
            (
                "COMPATIBILITY_ERROR",
                exit_codes::COMPATIBILITY,
                "The CLI and remote daemon did not agree on a supported protocol or version window.",
                Some("Use a compatible CLI/daemon version or rerun with `--no-compat-check` only after verifying the risk."),
            )
        } else if text.contains("remote") {
            (
                    "REMOTE_ERROR",
                    exit_codes::REMOTE,
                    "The requested remote was missing, unreachable, or returned an operation failure.",
                    Some("Run `claw remote list` to inspect configured remotes, or add one with `claw remote add`."),
                )
        } else if text.contains("uncommitted changes") {
            (
                    "WORKTREE_DIRTY",
                    exit_codes::WORKTREE_DIRTY,
                    "The command would overwrite or integrate over local worktree changes.",
                    Some("Run `claw status`, snapshot your work with `claw snapshot -m <message>`, or retry with `--force` when appropriate."),
                )
        } else if text.contains("conflict") || text.contains("merge in progress") {
            (
                    "CONFLICT_STATE",
                    exit_codes::CONFLICT,
                    "A merge or integration conflict is still open in this repository.",
                    Some("Run `claw resolve` to inspect conflicts, then `claw snapshot -m <message>` after resolving them."),
                )
        } else if text.contains("io error") || text.contains("permission denied") {
            (
                "IO_ERROR",
                exit_codes::IO,
                "The operating system rejected a file, directory, or process operation.",
                Some("Run `claw doctor`, then check filesystem permissions and that the target path is accessible."),
            )
        } else {
            (
                    "CLI_ERROR",
                    exit_codes::GENERAL,
                    "Claw hit an unclassified runtime error.",
                    Some("Run the command again with `--help`, or run `claw doctor` for local repository checks."),
                )
        };

        Self {
            schema_version: 1,
            code,
            message,
            reason,
            remediation,
            exit_code,
            details,
        }
    }

    pub fn print_human(&self, request_id: &str) {
        eprintln!(
            "error[{code}]: {message}",
            code = self.code,
            message = self.message
        );
        eprintln!("request_id: {request_id}");
        eprintln!("why: {}", self.reason);
        if let Some(remediation) = self.remediation {
            eprintln!("try: {remediation}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{exit_codes, CliDiagnostic};

    #[test]
    fn classifies_policy_failures() {
        let err = anyhow::anyhow!("policy 'release' denied revision abc");
        let diagnostic = CliDiagnostic::from_error(&err);

        assert_eq!(diagnostic.code, "POLICY_DENIED");
        assert_eq!(diagnostic.exit_code, exit_codes::POLICY);
    }

    #[test]
    fn classifies_compatibility_failures() {
        let err = anyhow::anyhow!("compatibility check failed for origin: incompatible");
        let diagnostic = CliDiagnostic::from_error(&err);

        assert_eq!(diagnostic.code, "COMPATIBILITY_ERROR");
        assert_eq!(diagnostic.exit_code, exit_codes::COMPATIBILITY);
    }

    #[test]
    fn classifies_protocol_negotiation_failures() {
        let err =
            anyhow::anyhow!("protocol negotiation failed: server protocol claw-sync/9 unsupported");
        let diagnostic = CliDiagnostic::from_error(&err);

        assert_eq!(diagnostic.code, "COMPATIBILITY_ERROR");
        assert_eq!(diagnostic.exit_code, exit_codes::COMPATIBILITY);
    }

    #[test]
    fn classifies_bearer_token_failures() {
        for message in ["missing bearer token", "invalid bearer token"] {
            let err = anyhow::anyhow!(message);
            let diagnostic = CliDiagnostic::from_error(&err);

            assert_eq!(diagnostic.code, "AUTH_ERROR");
            assert_eq!(diagnostic.exit_code, exit_codes::AUTH);
            assert!(diagnostic
                .remediation
                .expect("auth remediation")
                .contains("claw auth token set --stdin"));
        }
    }

    #[test]
    fn classifies_invalid_ref_names() {
        let err = anyhow::Error::new(claw_store::StoreError::InvalidRefName(
            "heads/CON".to_string(),
        ));
        let diagnostic = CliDiagnostic::from_error(&err);

        assert_eq!(diagnostic.code, "INVALID_REF_NAME");
        assert_eq!(diagnostic.exit_code, exit_codes::GENERAL);
        assert_eq!(diagnostic.details["ref_name"], "heads/CON");
    }

    #[test]
    fn classifies_ref_name_collisions() {
        let err = anyhow::Error::new(claw_store::StoreError::RefNameCollision {
            requested: "heads/MAIN".to_string(),
            existing: "heads/main".to_string(),
        });
        let diagnostic = CliDiagnostic::from_error(&err);

        assert_eq!(diagnostic.code, "REF_NAME_COLLISION");
        assert_eq!(diagnostic.exit_code, exit_codes::GENERAL);
        assert_eq!(diagnostic.details["requested"], "heads/MAIN");
        assert_eq!(diagnostic.details["existing"], "heads/main");
    }

    #[test]
    fn classifies_usage_failures() {
        let diagnostic = CliDiagnostic::from_usage_error(
            "unexpected argument '--wat'".to_string(),
            clap::error::ErrorKind::UnknownArgument,
        );

        assert_eq!(diagnostic.code, "USAGE_ERROR");
        assert_eq!(diagnostic.exit_code, exit_codes::USAGE);
        assert_eq!(diagnostic.details["kind"], "UnknownArgument");
    }
}
