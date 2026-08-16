# Contributing to heisensim

Thank you for your interest in contributing to heisensim! This guide will help you get started.

## Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (latest stable)
- [Nix](https://nixos.org/download/) (optional, for reproducible dev environment)

### Setup

```bash
git clone https://github.com/heisensim/heisensim.git
cd heisensim

# With Nix (recommended):
nix develop

# Without Nix:
cargo build
```

### Running Tests

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

Pre-commit hooks run automatically via [Lefthook](https://github.com/evilmartians/lefthook).

## Development Workflow

1. **Fork** the repository
2. **Branch** from `main`: `git checkout -b feat/my-feature`
3. **Implement** your changes
4. **Test**: `cargo test --workspace`
5. **Lint**: `cargo clippy --workspace -- -D warnings`
6. **Commit** with a descriptive message following [Conventional Commits](https://www.conventionalcommits.org/)
7. **Push** and open a Pull Request

## Project Structure

```
crates/
  cli/        # CLI entry point, subcommands, report rendering
  core/       # Core types: clock, seed, network, process
  fault/      # Fault scheduling, exploration, injection
  intercept/  # Syscall interception (future)
  k8s/        # Kubernetes client, pod discovery, fault ops
  probe/      # HTTP health probes
  props/      # SLA property evaluation (availability, latency, etc.)
  timeline/   # Event timeline, bus, queries
examples/
  k8s-demo/   # End-to-end demo with k3d
```

## Adding a New Fault Type

1. Add the variant to `FaultType` in `crates/fault/src/scheduler.rs`
2. Implement injection/reversion in `crates/k8s/src/fault_ops.rs`
3. Add match arms in `crates/cli/src/main.rs`
4. Update RBAC in `crates/cli/src/rbac.rs` if needed
5. Add tests

## Adding a New Property

1. Create `crates/props/src/my_property.rs`
2. Implement the `TimelineProperty` trait
3. Register in `crates/props/src/lib.rs`
4. Wire into `crates/cli/src/properties.rs`
5. Add tests (pass, fail, empty cases)

## Code of Conduct

Be kind. Be constructive. We're all here to build something great.

## License

By contributing, you agree that your contributions will be licensed under the same terms as the project (Apache-2.0 OR MIT).
