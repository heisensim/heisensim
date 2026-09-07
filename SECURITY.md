# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.15.x  | ✅ Current release |
| < 0.15  | ❌ No patches      |

## Reporting a Vulnerability

**heisensim** takes security seriously — especially given that it performs `ptrace` system call interception, `kubectl exec` into pods, and iptables/tc manipulation.

If you discover a security vulnerability, please report it responsibly:

1. **Do NOT open a public GitHub issue.**
2. Email **security@heisensim.dev** with:
   - Description of the vulnerability
   - Steps to reproduce
   - Affected versions
   - Potential impact
3. You will receive acknowledgment within **48 hours**.
4. We aim to provide a fix within **7 days** for critical issues.

## Scope

The following are in scope for security reports:

- **Container escape** via ptrace/vDSO injection
- **Privilege escalation** through debug containers or kubectl exec
- **SSRF** via probe URLs or Diverge integration
- **Namespace fence bypass** — fault injection outside allowed namespaces
- **Credential exposure** in logs, JSON output, or OTel traces
- **Dependency vulnerabilities** in direct dependencies

## Security Design

heisensim employs several safety mechanisms:

- **Namespace fencing** — blocks injection into `kube-system`, `kube-public`, and configurable blocked namespaces
- **No privileged containers** — uses ephemeral debug containers (no `--privileged`)
- **kill_on_drop** — all spawned kubectl processes are killed if the parent exits
- **Fault tracking** — `FaultTracker` ensures injected faults are reverted on shutdown
- **Dead Man's Switch** — automatic cleanup even on ungraceful termination
- **Dependency auditing** — `cargo deny` checks for known vulnerabilities

## Acknowledgments

We appreciate responsible disclosure and will credit reporters (with permission) in release notes.
