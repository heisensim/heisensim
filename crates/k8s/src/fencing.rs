//! Namespace fencing — prevents heisensim from operating in protected namespaces.
//!
//! This is a non-overridable safety boundary. Even if a user passes a protected
//! namespace via CLI flags, heisensim will refuse to inject faults.

use anyhow::Result;

/// Namespaces that heisensim must NEVER operate in.
const BLOCKED_NAMESPACES: &[&str] = &[
    "kube-system",
    "kube-public",
    "kube-node-lease",
    "istio-system",
    "linkerd",
    "cert-manager",
    "ingress-nginx",
    "monitoring",
    "gatekeeper-system",
    "calico-system",
    "tigera-operator",
];

/// Validate that a namespace is not in the blocklist.
///
/// Returns an error if the namespace is protected.
pub fn validate_namespace(namespace: &str) -> Result<()> {
    if BLOCKED_NAMESPACES.contains(&namespace) {
        anyhow::bail!(
            "Namespace '{}' is protected and cannot be targeted by heisensim.\n\
             Protected namespaces: {}\n\
             This restriction cannot be overridden.",
            namespace,
            BLOCKED_NAMESPACES.join(", ")
        );
    }
    Ok(())
}

/// Check if a namespace is blocked (without erroring).
pub fn is_blocked(namespace: &str) -> bool {
    BLOCKED_NAMESPACES.contains(&namespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blocked_namespaces() {
        assert!(validate_namespace("kube-system").is_err());
        assert!(validate_namespace("istio-system").is_err());
        assert!(validate_namespace("cert-manager").is_err());
    }

    #[test]
    fn test_allowed_namespaces() {
        assert!(validate_namespace("default").is_ok());
        assert!(validate_namespace("my-app").is_ok());
        assert!(validate_namespace("preview-pr-123").is_ok());
    }

    #[test]
    fn test_is_blocked() {
        assert!(is_blocked("kube-system"));
        assert!(!is_blocked("default"));
    }
}
