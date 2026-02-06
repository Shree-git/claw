use claw_core::types::{Capsule, Policy};

use crate::PolicyError;

pub fn verify_required_checks(policy: &Policy, capsule: &Capsule) -> Result<(), PolicyError> {
    for check in &policy.required_checks {
        let found = capsule
            .public_fields
            .evidence
            .iter()
            .any(|e| &e.name == check && e.status == "pass");
        if !found {
            return Err(PolicyError::MissingCheck(check.clone()));
        }
    }
    Ok(())
}
