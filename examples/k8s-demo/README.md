# Heisensim Demo: nginx + Redis on K3d

A simple 2-service app to demonstrate heisensim's K8s chaos testing.

## Architecture

```
┌──────────┐     ┌──────────┐
│  nginx   │────▶│  redis   │
│  (api)   │     │          │
│  :80     │     │  :6379   │
└──────────┘     └──────────┘
   2 replicas      1 replica
   HTTP probes     TCP + exec probes
```

## Prerequisites

- [Docker](https://docs.docker.com/get-docker/) running
- [k3d](https://k3d.io/) (`brew install k3d`)
- [kubectl](https://kubernetes.io/docs/tasks/tools/)
- heisensim (`cargo install --path crates/cli`)

## Quick Start

```bash
# 1. Create a K3d cluster
k3d cluster create heisensim-demo --wait

# 2. Deploy the demo app
kubectl apply -f examples/k8s-demo/manifests.yaml

# 3. Wait for pods
kubectl wait --for=condition=Ready pods --all -n heisensim-demo --timeout=60s

# 4. Run heisensim — auto-discovers probes, injects faults
heisensim run --namespace heisensim-demo --seed 42 --duration 2m

# 5. Replay the exact same run (same seed = same faults)
heisensim replay --seed 42 --namespace heisensim-demo --duration 2m

# 6. Clean up
k3d cluster delete heisensim-demo
```

## What Happens

1. **Discovery**: heisensim finds 2 deployments (api, redis) and auto-scrapes their K8s probe specs
2. **Probing**: HTTP probes hit nginx on `:80/`, TCP probes check redis on `:6379`, exec probe runs `redis-cli ping`
3. **Fault injection**: On a seeded schedule, heisensim randomly:
   - Crashes pods (`kubectl delete pod`)
   - Injects latency (`tc netem delay 300ms`)
4. **Reporting**: Timeline shows exactly when faults were injected and when probes detected failures

## Notes

- Pods have `NET_ADMIN` capability — required for `tc`/`iptables` fault injection
- The `--seed 42` flag makes the run deterministic — same faults, same order, same timing
- Use `heisensim init --namespace heisensim-demo` to generate a config file from the running cluster
