use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    Private,
    Restricted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub policy_id: String,
    #[serde(default)]
    pub required_checks: Vec<String>,
    #[serde(default)]
    pub required_reviewers: Vec<String>,
    #[serde(default)]
    pub sensitive_paths: Vec<String>,
    #[serde(default)]
    pub quarantine_lane: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_trust_score: Option<String>,
    /// Visibility is used by the policy evaluator for capsule enforcement.
    pub visibility: Visibility,
}
