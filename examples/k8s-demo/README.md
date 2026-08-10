# heisensim E2E Demo

Chaos test a multi-service Kubernetes app in under 5 minutes.

## What's in the box

| Component | Image | Replicas | Health Probes |
|:---|:---|:---:|:---|
| **api** | nginx:1.27-alpine | 2 | HTTP GET / (readiness + liveness) |
| **redis** | redis:7-alpine | 1 | TCP 6379 (readiness) + `redis-cli ping` (liveness) |

## Prerequisites

- [heisensim](https://heisensim.dev) (`brew install heisensim/tap/heisensim`)
- [k3d](https://k3d.io) (`brew install k3d`)
- [kubectl](https://kubernetes.io/docs/tasks/tools/)

## Quick Start

```bash
# Clone and run the demo
git clone https://github.com/heisensim/heisensim.git
cd heisensim/examples/k8s-demo
make all
```

That's it. `make all` will:
1. Create a k3d cluster with 2 agent nodes
2. Deploy the demo app (redis + 2× nginx)
3. Wait for all pods to be healthy
4. Run a 2-minute chaos test with seed 42

## What You'll See

### Fault Injection
heisensim automatically discovers pods and health probes, then injects:
- 💥 **Pod crashes** — deletes a running pod
- 🌐 **Network latency** — adds 200-700ms delay
- 🔌 **Network partitions** — iptables DROP between pods

### Property Verification
After faults, heisensim evaluates 5 SLA properties:

```
╔═══════════════════════════════════════════════════════════════╗
║  PROPERTY RESULTS                              4/5 PASS     ║
╠═══════════════════════════════════════════════════════════════╣
║  ✅ PASS  fast-recovery      recovery < 30s (actual: 8.2s)  ║
║  ✅ PASS  high-availability  avail ≥ 95% (actual: 97.1%)    ║
║  ✅ PASS  bounded-errors     max 5 consecutive (actual: 2)  ║
║  ✅ PASS  no-cascade         no cascading failures           ║
║  ✅ PASS  low-latency        p99 < 500ms (actual: 230ms)    ║
╚═══════════════════════════════════════════════════════════════╝
```

## Explore Mode

Run many seeds to find edge cases:

```bash
make explore
```

This runs 10 different random seeds, each with different fault timing:

```
  ✅ seed 0x0001  │  faults: 3  │  failures: 1  │  props: 5/5
  ❌ seed 0x0002  │  faults: 4  │  failures: 8  │  props: 3/5
  ✅ seed 0x0003  │  faults: 2  │  failures: 0  │  props: 5/5
```

## Replay a Bug

Found an interesting seed? Replay it deterministically:

```bash
heisensim run --namespace heisensim-demo --seed 0x0002 --duration 30s
```

Same seed → same faults → same results. Every time.

## Configuration

See [`heisensim.toml`](heisensim.toml) for the property definitions. Edit thresholds to see how your SLAs hold up:

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
- Run in CI: `heisensim explore --config heisensim.toml` exits 1 on failure
