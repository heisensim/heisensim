# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-08-10

### Added

- **Property Checking** — 5 timeline-aware invariant properties:
  - `recovery_time` — probes recover within N seconds after each fault
  - `availability` — probe success rate ≥ N%
  - `error_budget` — max consecutive failures per probe
  - `no_cascade` — faults don't cascade to unexpected probes
  - `latency_p99` — probe latency percentile ≤ threshold
- `[[properties]]` TOML config section for defining SLA properties
- Pretty verdict table printed after simulation
- Exit code 1 when any property fails (CI-friendly)
- Pre-built release binaries for Linux (x86_64, aarch64) and macOS (x86_64, Apple Silicon)
- Homebrew formula: `brew install heisensim/tap/heisensim`
- Nix flake: `nix run github:heisensim/heisensim`
- GitHub Actions release workflow (triggered on tag push)
- CHANGELOG.md
- Updated README with install instructions, property checking docs

### Changed

- Architecture section in README updated to reflect `heisensim-props` as active
- Roadmap updated: Phase 2 marked complete

## [0.1.1] - 2026-08-10

### Added

- Published to crates.io
- `heisensim explore` subcommand — run many seeds in parallel
- `heisensim init` subcommand — auto-generate config from K8s cluster
- `heisensim replay` subcommand — re-run with identical fault sequence
- Ephemeral container injection (`--inject-method debug`)
- gRPC health check probe support
- Exec probe support
- Automated publish workflow with token expiry validation
- README with badges, architecture, comparison matrix

### Fixed

- Workspace `Cargo.toml` version fields for crates.io publishing

## [0.1.0] - 2026-08-10

### Added

- Initial release
- K8s fault injection (pod crashes, network latency)
- Pod and probe auto-discovery
- HTTP and TCP health probe monitoring
- Microsecond event timeline with fault↔failure correlation
- Deterministic seed-based replay
- 8-crate workspace: cli, core, timeline, probe, k8s, fault, props, intercept

[0.2.0]: https://github.com/heisensim/heisensim/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/heisensim/heisensim/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/heisensim/heisensim/releases/tag/v0.1.0
