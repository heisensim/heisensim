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
- **kill_on_drop** — spawned kubectl processes are killed if the parent exits (applied to fault injection and debug container paths)
- **Fault tracking** — `FaultTracker` attempts to revert injected faults on shutdown; failed reverts are requeued for retry, but cleanup is best-effort and orphaned rules may require manual intervention
- **Dead Man's Switch** — best-effort cleanup on ungraceful termination via `AtomicBool` + `JoinSet`; not a hard guarantee under all crash scenarios
- **Dependency auditing** — `cargo deny` checks for known vulnerabilities

## Acknowledgments

We appreciate responsible disclosure and will credit reporters (with permission) in release notes.
