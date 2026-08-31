# heisensim

[![CI](https://github.com/heisensim/heisensim/actions/workflows/ci.yml/badge.svg)](https://github.com/heisensim/heisensim/actions)
[![Release](https://github.com/heisensim/heisensim/actions/workflows/release.yml/badge.svg)](https://github.com/heisensim/heisensim/releases)
[![Crates.io](https://img.shields.io/crates/v/heisensim.svg)](https://crates.io/crates/heisensim)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](LICENSE-MIT)
[![Open in GitHub Codespaces](https://github.com/codespaces/badge.svg)](https://codespaces.new/heisensim/heisensim?quickstart=1)

> **"Inject faults. Verify SLAs. In CI."**

**heisensim** is a chaos testing CLI for Kubernetes that injects faults, monitors health probes, and **verifies your SLA properties automatically**. Exit code 1 when properties fail — CI-native.

<p align="center">
  <img src="demo.gif" alt="heisensim terminal demo showing init preset, deterministic simulation with faults, multi-seed exploration in 22ms, and side-by-side seed diff" width="720">
</p>

```
╔═══════════════════════════════════════════════════════════════╗
║  PROPERTY RESULTS                                  4/5 PASS  ║
╠═══════════════════════════════════════════════════════════════╣
║  ✅ PASS  fast-recovery      recovery < 30s (actual: 8.2s)   ║
║  ❌ FAIL  high-availability  avail ≥ 99% (actual: 94.2%)     ║
║  ✅ PASS  bounded-errors     max 5 consecutive (actual: 2)   ║
║  ✅ PASS  no-cascade         no cascading failures            ║
║  ✅ PASS  low-latency        p99 < 500ms (actual: 230ms)     ║
╚═══════════════════════════════════════════════════════════════╝
```

---

## 📦 Install

### Homebrew (macOS / Linux)

```bash
brew install heisensim/tap/heisensim
```

### Nix

```bash
# Run directly
nix run github:heisensim/heisensim

# Or add to your flake
inputs.heisensim.url = "github:heisensim/heisensim";

# Dev shell (includes Rust toolchain, clippy, rustfmt)
nix develop github:heisensim/heisensim
```

### Cargo

```bash
cargo install heisensim
```

### Pre-built binaries

Download from [GitHub Releases](https://github.com/heisensim/heisensim/releases) — available for Linux (x86_64, aarch64) and macOS (x86_64, Apple Silicon).

### Docker

```bash
docker run ghcr.io/heisensim/heisensim:v0.15.0 simulate --seed 0x42 --duration 5m
```

### GitHub Action

```yaml
- uses: actions/checkout@v4
- uses: heisensim/action@v1
  with:
    config: heisensim.toml
```

See [heisensim/action](https://github.com/heisensim/action) for full docs.

---

## 🚀 Quick Start

### No cluster required — simulate mode

```bash
# 1. Generate a config
heisensim init --preset microservice

# 2. Run a deterministic simulation (completes in ~1ms)
heisensim simulate --seed 0x42 --duration 5m --config heisensim.toml

# 3. Explore 20 seeds — find every failure mode
heisensim explore --simulate --seeds 20 --duration 5m --config heisensim.toml
```

### With a Kubernetes cluster

```bash
# Full E2E demo with k3d
git clone https://github.com/heisensim/heisensim.git
cd heisensim/examples/k8s-demo
make all   # creates k3d cluster → deploys app → runs chaos test

# Or run against any namespace
heisensim run --namespace my-app --seed 42 --duration 2m --config heisensim.toml
```

> 📖 See the full [E2E demo guide](examples/k8s-demo/) for the complete walkthrough.


## 🛡️ Property Checking

The differentiator. Define SLA properties in TOML, heisensim evaluates them against the chaos test timeline:

```toml
# heisensim.toml

[[properties]]
name = "fast-recovery"
type = "recovery_time"
max_seconds = 30          # probes must recover within 30s of each fault

[[properties]]
name = "high-availability"
type = "availability"
min_percent = 99.0        # ≥99% probe success rate
probe_filter = "api"      # only evaluate api-* probes

[[properties]]
name = "bounded-errors"
type = "error_budget"
max_consecutive = 5       # no more than 5 consecutive failures per probe

[[properties]]
name = "no-cascade"
type = "no_cascade"
window_seconds = 30
allowed_failing_probes = ["redis"]  # redis probes expected to fail when redis is faulted

[[properties]]
name = "low-latency"
type = "latency_p99"
max_ms = 500              # p99 probe latency under 500ms
```

Properties produce verdicts with details:

```
  ❌ fast-recovery details:
    ❌ fault abc123 on redis: recovered in 47.2s (exceeds 30s)
    ✅ fault def456 on api: recovered in 3.1s
```

**Exit code 1** when any property fails — CI/CD friendly.

### Available Properties

| Property | What it checks | Required config |
|:---|:---|:---|
| `recovery_time` | Probes recover within N seconds after fault | `max_seconds` |
| `availability` | Probe success rate ≥ N% | `min_percent` |
| `error_budget` | Max consecutive failures per probe | `max_consecutive` |
| `no_cascade` | Faults don't cascade to unexpected probes | — |
| `latency_p99` | Probe latency at percentile ≤ threshold | `max_ms` |
| `dns_resolution` | DNS recovers within N seconds | `max_recovery_seconds` |
| `steady_state` | System returns to steady state after fault | `max_recovery_seconds`, `baseline_seconds` |
| `throughput` | Minimum requests per minute sustained | `min_per_minute`, `window_seconds` |

### A/B Baseline Diffing (v0.15.0+)

Don't know your SLA numbers? Use `--baseline` to automatically capture steady-state metrics during warmup and assert chaos-phase degradation stays within bounds:

```bash
# No SLA knowledge needed — heisensim measures your baseline automatically
heisensim run --namespace myapp --baseline

# With tuning:
#   --max-latency-multiplier  chaos p95 ≤ Nx baseline
#   --max-availability-drop   max Npp availability drop
#   --export-baseline         export snapshot for CI drift tracking
heisensim run --namespace myapp --baseline \
  --max-latency-multiplier 3.0 \
  --max-availability-drop 10.0 \
  --export-baseline baseline.json
```

Output:
```
📊 Baseline captured: 3 probes, 45 total samples
  http-check — p50: 42ms, p95: 68ms, avail: 100.0%
  grpc-ping  — p50: 12ms, p95: 28ms, avail: 100.0%
  dns-lookup — p50: 8ms, p95: 15ms, avail: 99.2%

📊 Evaluating baseline diff properties...
  ✅ http-check: p95 1.8x (baseline 68ms → chaos 122ms)
  ✅ grpc-ping: p95 2.1x (baseline 28ms → chaos 59ms)
  ❌ dns-lookup: p95 4.2x (baseline 15ms → chaos 63ms)
```

### Diverge Preview Environment Integration (v0.15.0+)

Run chaos tests against [Diverge](https://diverge.dev) preview environments with blast-radius targeting:

```bash
# Chaos test only changed services + upstream callers in PR preview
heisensim diverge run pr-123 --baseline --soft-fail

# With explicit config
heisensim diverge run my-feature-env \
  --url https://diverge.example.com \
  --duration 2m \
  --baseline
```

---

## ✨ Features

- **Deterministic simulation engine (DST)** — runs entirely in-memory, no cluster required. Same seed = same hash.
- **Auto-discovers K8s pods and health probes** — scrapes readiness/liveness probes from pod specs
- **Injects faults** — pod crashes (`kubectl delete`), network latency (`tc netem`), partitions, DNS failures, stress
- **Ephemeral container injection** — works with distroless images via `kubectl debug`
- **Monitors health probes during faults** — HTTP, TCP, gRPC, exec probes
- **Correlates faults to failures** — microsecond-precision event timeline
- **OpenTelemetry correlation** — links fault spans to your application traces via `traceparent`
- **Property checking** — verify SLA invariants automatically (8 built-in properties)
- **Explore mode** — run many seeds in parallel to find interesting failures
- **Seed bisection** — binary search for the minimal failing seed (`--bisect`)
- **Seed diff** — compare two seeds side-by-side (`heisensim diff`)
- **Init presets** — battle-tested configs for microservices, stateful workloads, and CI
- **CI-native** — GitHub Action, GitLab CI template, JUnit XML, JSON output
- **Process-Level Fault Injection** — `heisensim process-fault` attaches to running processes via ptrace, intercepts syscalls, and injects faults without containers or kernels modules
- **Multi-Thread Tracing** — Automatically traces all threads (Go goroutines, Tokio workers, JVM thread pools) via `/proc/PID/task/` enumeration and `PTRACE_O_TRACECLONE`
- **Port Filtering** — `--port 5432` reads sockaddr from process memory to target specific connections (e.g. fault Postgres without breaking DNS)
- **Connect Latency** — `--fault connect-latency --latency 200` adds realistic latency to `connect()` calls
- **Property Templates** — `--property-template three-nines` loads pre-built SLA property bundles

---

## 💻 CLI Reference

### `heisensim run`

Run a chaos test against a live Kubernetes cluster:

```bash
heisensim run --namespace demo --seed 42 --duration 2m --config heisensim.toml
```

| Flag | Default | Description |
|:---|:---|:---|
| `--namespace` | `default` | Target K8s namespace |
| `--duration` | `5m` | Test duration |
| `--seed` | random | Deterministic seed |
| `--config` | — | TOML config with `[[properties]]` |
| `--warmup` | `30s` | Warmup before faults start |
| `--faults` | `crash,latency` | Comma-separated fault types |
| `--inject-method` | `exec` | `exec` or `debug` (ephemeral containers) |
| `--output` | `terminal` | Output format: `terminal`, `json`, `markdown` |
| `--otel-endpoint` | — | OTLP endpoint for trace correlation |
| `--k3d` | — | Spin up ephemeral K3d cluster |

### `heisensim simulate`

Run a deterministic in-memory chaos simulation. No Kubernetes cluster required.
Same seed always produces the same timeline hash.

```bash
# Basic simulation
heisensim simulate --seed 0x42 --duration 5m

# With config-driven properties
heisensim simulate --seed 0x42 --duration 5m --config heisensim.toml

# JSON output for CI pipelines
heisensim simulate --seed 0x42 --duration 5m --output json

# JUnit XML for test reporters
heisensim simulate --seed 0x42 --duration 5m --output junit > results.xml

# Custom fault mix and pod count
heisensim simulate --seed 0x42 --duration 10m --faults crash,latency,partition --pods 5
```

| Flag | Default | Description |
|:---|:---|:---|
| `--seed` | random | Simulation seed (hex `0xBEEF` or decimal `42`) |
| `--duration` | `5m` | Virtual simulation duration |
| `--warmup` | `30s` | Warmup period before faults begin |
| `--faults` | all | Comma-separated fault types |
| `--pods` | `3` | Number of simulated pods |
| `--config` | — | TOML config with `[[properties]]` |
| `--time-scale` | `instant` | Playback speed (`100x`, `10x`, `instant`) |
| `--output` | `text` | Output format (`text`, `json`, `junit`, `html`) |
| `--profile` | none | Fault preset (`standard`, `aggressive`) |

### `heisensim explore`

Run many seeds to find bugs. Works in both live and simulate modes:

```bash
# Simulate mode (no cluster required)
heisensim explore --simulate --seeds 50 --duration 5m --config heisensim.toml

# With seed bisection — find the minimal failing seed
heisensim explore --simulate --seeds 100 --duration 5m --config heisensim.toml --bisect

# Live K8s mode
heisensim explore --namespace demo --seeds 50 --parallel 5 --duration 30s
```

| Flag | Default | Description |
|:---|:---|:---|
| `--simulate` | — | Run in deterministic simulation mode |
| `--seeds` | `10` | Number of seeds to explore |
| `--duration` | `5m` | Duration per seed |
| `--config` | — | TOML config with `[[properties]]` |
| `--bisect` | — | Binary search for minimal failing seed |
| `--parallel` | `5` | Parallel workers (live mode) |
| `--output` | `text` | Output format (`text`, `json`) |

### `heisensim init`

Generate a config file with battle-tested defaults:

```bash
# Default basic config
heisensim init

# Microservice mesh preset (5 SLA properties, 4 fault types)
heisensim init --preset microservice

# Stateful workload preset (database/queue testing)
heisensim init --preset stateful

# CI-optimized quick smoke test
heisensim init --preset ci

# Auto-generate from running cluster
heisensim init --namespace demo
```

| Flag | Default | Description |
|:---|:---|:---|
| `--preset` | `basic` | Config template: `basic`, `microservice`, `stateful`, `ci` |
| `--namespace` | — | Auto-discover probes from K8s namespace |
| `--output` | `heisensim.toml` | Output file path |
| `--dry-run` | — | Preview without writing |

### `heisensim diff`

Compare two simulation seeds side-by-side:

```bash
# Compare seeds
heisensim diff --seed-a 0x01 --seed-b 0x02 --duration 5m --config heisensim.toml

# JSON output
heisensim diff --seed-a 0x01 --seed-b 0x02 --duration 5m --output json
```

| Flag | Default | Description |
|:---|:---|:---|
| `--seed-a` | required | First seed |
| `--seed-b` | required | Second seed |
| `--duration` | `5m` | Simulation duration |
| `--config` | — | TOML config with `[[properties]]` |
| `--faults` | all | Comma-separated fault types |
| `--output` | `text` | Output format (`text`, `json`) |

### `heisensim replay`

Re-run a previous test with identical fault sequence:

```bash
heisensim replay --seed 42 --namespace demo
```

### `heisensim process-fault`

```bash
heisensim process-fault --pid 1234 --fault connect-error --errno 111 --duration 30s
heisensim process-fault --pid 1234 --fault fd-exhaustion --duration 10s
heisensim process-fault --pid 1234 --fault connect-latency --latency 200 --port 5432 --duration 60s
```

Flags: `--pid`, `--fault` (connect-error|fd-exhaustion|connect-latency), `--errno`, `--latency`, `--port`, `--duration`.
Notes: Requires Linux x86_64. Attaches to all threads automatically. `--port` cannot be combined with `fd-exhaustion`.

---

## 🔌 CI Integration

### GitHub Actions

```yaml
- uses: actions/checkout@v4
- uses: heisensim/action@v1
  with:
    config: heisensim.toml
    seeds: '50'
    bisect: 'true'
```

### GitLab CI

```yaml
include:
  - remote: 'https://raw.githubusercontent.com/heisensim/heisensim/main/examples/ci/gitlab-template.yml'

chaos-test:
  extends: .heisensim-chaos-test
  variables:
    HEISENSIM_CONFIG: heisensim.toml
```

See [`examples/ci/`](examples/ci/) for copy-paste configs.

---

## 🏗️ Architecture

```text
heisensim/
├── crates/
│   ├── cli/         # CLI binary & orchestration
│   ├── timeline/    # Microsecond event bus & correlation
│   ├── probe/       # Async probe runners (HTTP, TCP, gRPC, exec)
│   ├── k8s/         # K8s client, discovery & fault operators
│   ├── fault/       # Fault scheduling, PRNG & DST engine
│   ├── props/       # Property checking (8 timeline-aware invariants)
│   ├── core/        # Core types, config & virtual clock
│   └── intercept/   # (Future: syscall interception)
```

---

## ⚖️ Comparison

| Feature | heisensim | Chaos Monkey | Litmus | Chaos Mesh | Gremlin |
| :--- | :---: | :---: | :---: | :---: | :---: |
| **No Cluster Required** | ✅ | ❌ | ❌ | ❌ | ❌ |
| **SLA Property Checking** | ✅ | ❌ | ❌ | ❌ | ❌ |
| **Deterministic Replay** | ✅ | ❌ | ❌ | ❌ | ❌ |
| **Seed Bisection & Diff** | ✅ | ❌ | ❌ | ❌ | ❌ |
| **Auto-Discovery & Probing** | ✅ | ❌ | ⚠️ | ⚠️ | ❌ |
| **Zero-CRD CLI** | ✅ | ❌ | ❌ | ❌ | ❌ |
| **Fault↔Failure Correlation** | ✅ | ❌ | ❌ | ❌ | ❌ |
| **GitHub Action / GitLab CI** | ✅ | ❌ | ❌ | ❌ | ⚠️ |
| **Open Source** | ✅ | ✅ | ✅ | ✅ | ❌ |

---

## 🗺️ Roadmap

- `v0.10.0` ✅ Explore, bisect, diff, init presets, CI integration
- `v0.11.0` ✅ vDSO time manipulation, process fault injection engine
- `v0.12.0` ✅ Property templates, connect-latency, port filtering, multi-thread tracing
- `v0.13.0` ✅ `--name` process targeting, aarch64 ptrace support
- `v0.14.0` ✅ Grafana dashboard, streaming OTel metrics, one-command observability stack
- `v0.15.0` ✅ A/B baseline diffing, Diverge preview env integration, fault tracker + graceful shutdown
- Future: mdbook docs site, golden baseline import, dead man's switch TTL

---

## 📜 License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT License ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

---

## 🤝 Contributing

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
