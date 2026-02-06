use claw_core::types::{Capsule, Policy, Revision};

use crate::checks::verify_required_checks;
use crate::visibility::check_visibility;
use crate::PolicyError;

pub fn evaluate_policy(
    policy: &Policy,
    _revision: &Revision,
    capsule: &Capsule,
) -> Result<(), PolicyError> {
    // Check visibility constraints
    check_visibility(policy, capsule)?;

    // Check required checks
    verify_required_checks(policy, capsule)?;

    Ok(())
}
