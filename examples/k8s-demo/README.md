# heisensim E2E Demo

Verify your SLAs under chaos — in 5 minutes.

## What's in the box

| Component | Image | Replicas | Health Probes |
|:---|:---|:---:|:---|
| **api** | nginx:1.27-alpine | 2 | HTTP GET / (readiness + liveness) |
| **redis** | redis:7-alpine | 1 | TCP 6379 (readiness) + `redis-cli ping` (liveness) |

## Prerequisites

- [Docker](https://docs.docker.com/get-docker/), [OrbStack](https://orbstack.dev), or [Colima](https://github.com/abiosoft/colima) (container runtime for k3d)
- [heisensim](https://heisensim.dev) (`brew install heisensim/tap/heisensim`)
- [k3d](https://k3d.io) (`brew install k3d`)
- [kubectl](https://kubernetes.io/docs/tasks/tools/)

## Quick Start

```bash
git clone https://github.com/heisensim/heisensim.git
cd heisensim/examples/k8s-demo
make all
```

That's it. `make all` will:
1. Create a k3d cluster with 2 agent nodes
2. Deploy the demo app (redis + 2× nginx)
3. Wait for all pods to be healthy
4. Generate and apply least-privilege RBAC
5. Run a 30-second chaos test with seed 42

## What You'll See

### Fault Injection
heisensim automatically discovers pods and health probes, then injects:
- 💥 **Pod crashes** — deletes a running pod
- 🌐 **Network latency** — adds 200-700ms delay
- 🔌 **Network partitions** — iptables DROP between pods

### Property Verification
After faults, heisensim evaluates 5 SLA properties defined in [`heisensim.toml`](heisensim.toml):

```text
╔═══════════════════════════════════════════════════════════════╗
║  PROPERTY RESULTS                              5/5 PASS      ║
╠═══════════════════════════════════════════════════════════════╣
║  ✅ PASS  fast-recovery      recovery < 30s (actual: 8.2s)  ║
║  ✅ PASS  high-availability  avail ≥ 95% (actual: 97.1%)    ║
║  ✅ PASS  bounded-errors     max 5 consecutive (actual: 2)   ║
║  ✅ PASS  no-cascade         no cascading failures            ║
║  ✅ PASS  low-latency        p99 < 500ms (actual: 230ms)     ║
╚═══════════════════════════════════════════════════════════════╝
```

## All Make Targets

| Target | What it does |
|:-------|:-------------|
| `make all` | Full pipeline: setup → deploy → wait → rbac → run |
| `make run` | Single chaos test (seed 42, 30s) |
| `make explore` | 10 random seeds — find edge cases |
| `make rbac` | Generate & apply least-privilege K8s RBAC |
| `make junit` | Run and save JUnit XML report |
| `make json` | Run and save JSON report |
| `make clean` | Delete cluster and reports |

## Explore Mode

Run many seeds to find edge cases:

```bash
make explore
```

This runs 10 different random seeds, each with different fault timing:

```text
  ✅ seed 0x0001  │  faults: 3  │  failures: 1  │  props: 5/5
  ❌ seed 0x0002  │  faults: 4  │  failures: 8  │  props: 3/5
  ✅ seed 0x0003  │  faults: 2  │  failures: 0  │  props: 5/5
```

## CI Integration

### JUnit XML (for test runners)

```bash
make junit
# → heisensim-report.xml
```

```xml
<testsuite name="heisensim" tests="5" failures="0">
  <testcase name="fast-recovery" classname="heisensim.properties">
    <system-out>recovery &lt; 30s (actual: 8.2s)</system-out>
  </testcase>
  ...
</testsuite>
```

### JSON (for dashboards and pipelines)

```bash
make json
# → heisensim-report.json
```

### RBAC (least-privilege)

```bash
make rbac
```

Generates a `ServiceAccount`, `Role`, and `RoleBinding` scoped to exactly the permissions heisensim needs — nothing more:

```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: heisensim-role
  namespace: heisensim-demo
rules:
- apiGroups: [""]
  resources: ["pods"]
  verbs: ["get", "list", "delete"]
- apiGroups: [""]
  resources: ["pods/exec"]
  verbs: ["create"]
```

## Replay a Bug

Found an interesting seed? Replay it deterministically:

```bash
heisensim run --namespace heisensim-demo --seed 0x0002 --duration 30s
```

Same seed → same faults → same results. Every time.

## Configuration

Edit [`heisensim.toml`](heisensim.toml) to tune property thresholds:

```toml
[[properties]]
name = "fast-recovery"
type = "recovery_time"
max_seconds = 30   # ← try 10 to see it fail
```

## Clean Up

```bash
make clean
```

## Next Steps

- Try it on your own namespace: `heisensim run --namespace your-app`
- Add stricter properties to `heisensim.toml`
- Run in CI: `heisensim explore --config heisensim.toml` (exits 1 on failure)
- Export metrics: `--otel-endpoint http://localhost:4318` for Grafana dashboards
- Generate RBAC: `heisensim rbac --namespace your-app --faults crash,latency`
