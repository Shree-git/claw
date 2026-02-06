use claw_core::types::{Capsule, Policy, Visibility};

use crate::PolicyError;

pub fn check_visibility(policy: &Policy, capsule: &Capsule) -> Result<(), PolicyError> {
    match policy.visibility {
        Visibility::Public => Ok(()),
        Visibility::Private => {
            if capsule.encrypted_private.is_none() {
                return Err(PolicyError::Violation(
                    "private policy requires encrypted private fields".into(),
                ));
            }
            Ok(())
        }
        Visibility::Restricted => {
            // In MVP, restricted just means encrypted private must exist
            if capsule.encrypted_private.is_none() {
                return Err(PolicyError::Violation(
                    "restricted policy requires encrypted private fields".into(),
                ));
            }
            Ok(())
        }
    }
}
