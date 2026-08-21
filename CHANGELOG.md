# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.9.0] - 2026-08-19

### Added
- **Deterministic Simulation Engine**: New `heisensim simulate` subcommand for shift-left chaos testing
  - Same seed → same timeline hash → reproducible results
  - Discrete-event loop using `VirtualClock<T>` with priority ordering
  - Simulates 5 minutes of chaos in <500ms wall time
  - VirtualNetwork integration for realistic probe routing
  - Timeline hash (xxh64) for determinism proof
  - `--time-scale` for watchable terminal output
- Generic `Timer<T>` in VirtualClock with priority-based ordering
- `VirtualNetwork::new_seeded()` and `has_partition_between()` for deterministic network simulation
- `VirtualTime::as_std_duration()` bridge method

### Changed
- `ProcessTable` uses `BTreeMap` for deterministic iteration order
- `VirtualNetwork` partitions use `BTreeSet` for deterministic behavior
- `--mock` flag is now deprecated in favor of `heisensim simulate`

### Fixed  
- Deterministic UUIDs (seeded RNG instead of OS entropy)
- Deterministic timestamps (fixed epoch instead of wall clock)

## [0.7.0] - 2026-08-14

### Added
- Mock mode (`--mock`) for cluster-free simulation
- Discussion templates (Ideas, Q&A, Show & Tell)
- Public roadmap issues (#25-#30)

## [0.6.0] - 2026-08-13

### Added
- Explore strategies: `--explore-strategy sequential|random|coverage`
- Seed bisection: `--bisect` finds nearest failing seed
- HTML timeline report: `--output html` with dark-theme visualization
- ValueEnum for strategy argument (invalid values now error)
- HTML escaping for dynamic report values
- Parallel validation (`--parallel 0` now errors)

## [0.5.0] - 2026-08-13

### Added
- Stress fault type (CPU/memory via stress-ng ephemeral containers)
- DNS fault type (iptables port 53 blocking)
- GitHub Actions composite action for CI integration
- Three new property types: throughput, steady-state, dns-resolution
- Input validation for property constructors
- Fault ID tracking in steady-state evaluation

### Fixed
- Flaky traceparent integration test (OTel global propagator race)
- Atomic DNS iptables cleanup

## [0.4.0] - 2026-08-09

### Added
- Initial public release
- Core fault types: network-delay, network-loss, network-partition
- Property system: availability, latency, recovery, error-budget, cascade, no-crash, no-hang
- Explore mode with parallel seed testing
- JSON and JUnit output formats
- OTel tracing integration
- Deterministic seed-based fault scheduling
- k8s-demo example with k3d
- Homebrew tap installation
- GitHub Actions CI/CD

[0.9.0]: https://github.com/heisensim/heisensim/compare/v0.7.0...v0.9.0
[0.7.0]: https://github.com/heisensim/heisensim/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/heisensim/heisensim/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/heisensim/heisensim/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/heisensim/heisensim/releases/tag/v0.4.0
