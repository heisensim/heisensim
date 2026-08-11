pub fn generate_rbac(namespace: &str, faults: &[String], service_account_name: &str) -> String {
    let mut pod_verbs = vec!["get", "list"];
    let mut has_exec = false;
    let mut has_ephemeral = false;

    for fault in faults {
        let fault = fault.trim();
        if fault == "crash" && !pod_verbs.contains(&"delete") {
            pod_verbs.push("delete");
        }
        if fault == "latency" || fault == "partition" {
            has_exec = true;
        }
        if fault == "debug" {
            has_ephemeral = true;
        }
    }

    let mut rules = format!(
        "  - apiGroups: [\"\"]\n    resources: [\"pods\"]\n    verbs: [{}]\n",
        pod_verbs
            .iter()
            .map(|s| format!("\"{}\"", s))
            .collect::<Vec<_>>()
            .join(", ")
    );

    if has_exec {
        rules.push_str(
            "  - apiGroups: [\"\"]\n    resources: [\"pods/exec\"]\n    verbs: [\"create\"]\n",
        );
    }

    if has_ephemeral {
        rules.push_str("  - apiGroups: [\"\"]\n    resources: [\"pods/ephemeralcontainers\"]\n    verbs: [\"update\"]\n");
    }

    format!(
        "apiVersion: v1\n\
        kind: ServiceAccount\n\
        metadata:\n  \
          name: {sa}\n  \
          namespace: {ns}\n\
        ---\n\
        apiVersion: rbac.authorization.k8s.io/v1\n\
        kind: Role\n\
        metadata:\n  \
          name: {sa}-role\n  \
          namespace: {ns}\n\
        rules:\n\
        {rules}\
        ---\n\
        apiVersion: rbac.authorization.k8s.io/v1\n\
        kind: RoleBinding\n\
        metadata:\n  \
          name: {sa}-binding\n  \
          namespace: {ns}\n\
        roleRef:\n  \
          apiGroup: rbac.authorization.k8s.io\n  \
          kind: Role\n  \
          name: {sa}-role\n\
        subjects:\n\
        - kind: ServiceAccount\n  \
          name: {sa}\n  \
          namespace: {ns}\n",
        sa = service_account_name,
        ns = namespace,
        rules = rules,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rbac_crash_only() {
        let yaml = generate_rbac("default", &["crash".to_string()], "heisensim");
        assert!(yaml.contains("\"delete\""));
        assert!(!yaml.contains("pods/exec"));
    }

    #[test]
    fn test_rbac_latency() {
        let yaml = generate_rbac("default", &["latency".to_string()], "heisensim");
        assert!(yaml.contains("pods/exec"));
    }

    #[test]
    fn test_rbac_all_faults() {
        let yaml = generate_rbac(
            "default",
            &[
                "crash".to_string(),
                "latency".to_string(),
                "debug".to_string(),
            ],
            "heisensim",
        );
        assert!(yaml.contains("\"delete\""));
        assert!(yaml.contains("pods/exec"));
        assert!(yaml.contains("pods/ephemeralcontainers"));
    }

    #[test]
    fn test_rbac_valid_yaml() {
        let yaml = generate_rbac("default", &["crash".to_string()], "heisensim");
        assert!(yaml.starts_with("apiVersion: v1"));
        assert!(yaml.contains("kind: Role"));
        assert!(yaml.contains("kind: RoleBinding"));
    }
}
