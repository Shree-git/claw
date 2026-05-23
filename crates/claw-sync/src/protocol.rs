pub const SYNC_PROTOCOL_VERSION: &str = "claw-sync/1";
pub const CAP_PROTOCOL_V1: &str = "protocol:claw-sync/1";
pub const CAP_PARTIAL_CLONE: &str = "partial-clone";
pub const CAP_EVENT_BUS: &str = "event-bus";
pub const CAP_REQUEST_LIMITS: &str = "request-limits";
pub const CAP_HOSTED_HTTP: &str = "hosted-http";
pub const CAP_POLICY_AWARE_PUSH: &str = "policy-aware-push";

pub const SERVER_CAPABILITIES: &[&str] = &[
    CAP_PROTOCOL_V1,
    CAP_PARTIAL_CLONE,
    CAP_EVENT_BUS,
    CAP_REQUEST_LIMITS,
];

pub fn server_capabilities() -> Vec<String> {
    SERVER_CAPABILITIES
        .iter()
        .map(|capability| (*capability).to_string())
        .collect()
}

pub fn negotiate_capabilities(client_capabilities: &[String]) -> Vec<String> {
    if client_capabilities.is_empty() {
        return server_capabilities();
    }

    let mut negotiated = vec![CAP_PROTOCOL_V1.to_string()];
    negotiated.extend(
        SERVER_CAPABILITIES
            .iter()
            .filter(|server_capability| **server_capability != CAP_PROTOCOL_V1)
            .filter(|server_capability| {
                client_capabilities
                    .iter()
                    .any(|client_capability| client_capability == **server_capability)
            })
            .map(|capability| (*capability).to_string()),
    );
    negotiated
}

pub fn negotiated_protocol_version(capabilities: &[String]) -> Option<&'static str> {
    capabilities
        .iter()
        .any(|capability| capability == CAP_PROTOCOL_V1)
        .then_some(SYNC_PROTOCOL_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_client_capabilities_get_compatibility_baseline() {
        assert_eq!(
            negotiate_capabilities(&[]),
            vec![
                CAP_PROTOCOL_V1,
                CAP_PARTIAL_CLONE,
                CAP_EVENT_BUS,
                CAP_REQUEST_LIMITS
            ]
        );
    }

    #[test]
    fn negotiation_returns_supported_intersection_in_server_order() {
        let negotiated = negotiate_capabilities(&[
            "unknown".to_string(),
            CAP_REQUEST_LIMITS.to_string(),
            CAP_PARTIAL_CLONE.to_string(),
        ]);

        assert_eq!(
            negotiated,
            vec![CAP_PROTOCOL_V1, CAP_PARTIAL_CLONE, CAP_REQUEST_LIMITS]
        );
        assert_eq!(
            negotiated_protocol_version(&negotiated),
            Some(SYNC_PROTOCOL_VERSION)
        );
    }
}
