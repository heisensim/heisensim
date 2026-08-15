# heisensim

[![CI](https://github.com/heisensim/heisensim/actions/workflows/ci.yml/badge.svg)](https://github.com/heisensim/heisensim/actions)
[![Release](https://github.com/heisensim/heisensim/actions/workflows/release.yml/badge.svg)](https://github.com/heisensim/heisensim/releases)
[![Crates.io](https://img.shields.io/crates/v/heisensim.svg)](https://crates.io/crates/heisensim)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg)](LICENSE-MIT)
[![Open in GitHub Codespaces](https://github.com/codespaces/badge.svg)](https://codespaces.new/heisensim/heisensim?quickstart=1)

> **"Inject faults. Verify SLAs. In CI."**

**heisensim** is a chaos testing CLI for Kubernetes that injects faults, monitors health probes, and **verifies your SLA properties automatically**. Exit code 1 when properties fail — CI-native.

<p align="center">
  <img src="demo.gif" alt="heisensim demo" width="720">
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

---

## 🚀 Quick Start

```bash
git clone https://github.com/heisensim/heisensim.git
cd heisensim/examples/k8s-demo
make all   # creates k3d cluster → deploys app → runs chaos test
```

Or step by step:

```bash
# Single chaos test with property checking
heisensim run --namespace heisensim-demo --seed 42 --duration 2m --config heisensim.toml

# Explore 10 seeds in parallel
heisensim explore --namespace heisensim-demo --seeds 10 --duration 30s --config heisensim.toml

# Replay exact same run
heisensim replay --seed 42 --namespace heisensim-demo --duration 2m
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

---

## ✨ Features

- **Auto-discovers K8s pods and health probes** — scrapes readiness/liveness probes from pod specs
- **Injects faults** — pod crashes (`kubectl delete`), network latency (`tc netem`), partitions
- **Ephemeral container injection** — works with distroless images via `kubectl debug`
- **Monitors health probes during faults** — HTTP, TCP, gRPC, exec probes
- **Correlates faults to failures** — microsecond-precision event timeline
- **OpenTelemetry correlation** — links fault spans to your application traces via `traceparent`
- **Deterministic replay** — same seed = same faults = same results
- **Property checking** — verify SLA invariants automatically
- **Explore mode** — run many seeds in parallel to find interesting failures
- **JSON output** — `--output json` for CI pipeline integration

---

## 💻 CLI Reference

### `heisensim run`

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

### `heisensim explore`

Run many seeds in parallel to find bugs:

```bash
heisensim explore --namespace demo --seeds 50 --parallel 5 --duration 30s
```

### `heisensim init`

Auto-generate config from running cluster:

```bash
heisensim init --namespace demo --output heisensim.toml

# Preview without writing
heisensim init --namespace demo --dry-run
```

### `heisensim replay`

Re-run a previous test with identical fault sequence:

```bash
heisensim replay --seed 42 --namespace demo
```

---

## 🏗️ Architecture

```text
heisensim/
├── crates/
│   ├── cli/         # CLI binary & orchestration
│   ├── timeline/    # Microsecond event bus & correlation
│   ├── probe/       # Async probe runners (HTTP, TCP, gRPC, exec)
│   ├── k8s/         # K8s client, discovery & fault operators
│   ├── fault/       # Fault scheduling & PRNG
│   ├── props/       # Property checking (5 timeline-aware invariants)
│   ├── core/        # Core types & config
│   └── intercept/   # (Future: syscall interception)
```

---

## ⚖️ Comparison

| Feature | heisensim | Chaos Monkey | Litmus | Chaos Mesh | Gremlin |
| :--- | :---: | :---: | :---: | :---: | :---: |
| **SLA Property Checking** | ✅ | ❌ | ❌ | ❌ | ❌ |
| **Deterministic Replay** | ✅ | ❌ | ❌ | ❌ | ❌ |
| **Auto-Discovery & Probing** | ✅ | ❌ | ⚠️ | ⚠️ | ❌ |
| **Zero-CRD CLI** | ✅ | ❌ | ❌ | ❌ | ❌ |
| **Fault↔Failure Correlation** | ✅ | ❌ | ❌ | ❌ | ❌ |
| **Open Source** | ✅ | ✅ | ✅ | ✅ | ❌ |

---

## 🗺️ Roadmap

- **Phase 1 ✅**: K8s fault injection, auto-discovery, timeline correlation, deterministic replay
- **Phase 2 ✅**: Property checking, explore mode, ephemeral container injection, gRPC probes
- **Phase 2.5 ✅**: OpenTelemetry correlation, JSON output, CI pipeline support, crates.io publish
- **Phase 3 🔜**: GitHub Action (`uses: heisensim/action@v1`), docs site, eBPF network partitions
- **Phase 4 📋**: Process-level determinism (seccomp-BPF / ptrace)

---

## 📜 License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT License ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

---

## 🤝 Contributing

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
