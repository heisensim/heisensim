# Contributing to heisensim

Thank you for your interest in contributing to **heisensim**! We welcome contributions of all kinds, including bug fixes, feature implementations, documentation improvements, and bug reports.

## Finding Work

A great place to start is by checking out open issues on GitHub:
- [Good First Issues](https://github.com/heisensim/heisensim/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22) — issues suited for newcomers.
- [Help Wanted Issues](https://github.com/heisensim/heisensim/issues?q=is%3Aissue+is%3Aopen+label%3A%22help+wanted%22) — tasks where community help is appreciated.
- [All Open Issues](https://github.com/heisensim/heisensim/issues) — browse all open issues and feature proposals.

If you'd like to work on a new feature or major change, please open an issue first to discuss your proposal with the maintainers.

---

## Development Environment Setup

You can set up your development environment either using **Nix** (recommended) or manually with standard **Rust** tools.

### Option A: Using Nix (Recommended)

If you have Nix with flakes enabled, entering the development shell provides a complete toolchain with Rust, Clippy, rustfmt, and Git hooks automatically configured:

```bash
nix develop
```

### Option B: Manual Setup

1. **Rust Toolchain**: Ensure you have stable Rust installed via [rustup](https://rustup.rs/):
   ```bash
   rustup default stable
   rustup component add clippy rustfmt
   ```
2. **Git Hooks**: We use [Lefthook](https://github.com/evilmartians/lefthook) for git hooks. If installed, Lefthook auto-installs git hooks (`pre-commit` and `pre-push`) when entering the repository or running:
   ```bash
   lefthook install
   ```

---

## Development Workflow

### Building

Build all crates in the workspace:

```bash
cargo build --workspace
```

### Testing

Run the full test suite across the workspace:

```bash
cargo test --workspace
```

### Formatting & Linting

Ensure code adheres to format and lint checks before submitting a pull request:

```bash
# Format code
cargo fmt --all

# Check for lint errors
cargo clippy --workspace -- -D warnings
```

---

## Pull Request Process

1. **Fork & Clone**: Fork the repository on GitHub and clone your fork locally.
2. **Create a Branch**: Create a feature or bugfix branch off `main`:
   ```bash
   git checkout -b feat/your-feature-name
   ```
3. **Commit Changes**: Use [Conventional Commits](https://www.conventionalcommits.org/) for clean commit history:
   - `feat: add SLA probe verification for HTTP endpoints`
   - `fix: handle pod termination timeouts gracefully`
   - `docs: update setup instructions in README`
   - `test: add unit tests for fault partition logic`
4. **Verify**: Ensure all tests pass, lints pass (`cargo clippy --workspace -- -D warnings`), and formatting is clean (`cargo fmt --all`).
5. **Submit PR**: Push your branch to your fork and submit a Pull Request against the `main` branch of the `heisensim/heisensim` repository.

---

## Code Style & Best Practices

- **Follow Existing Patterns**: Align with the project's idiomatic Rust conventions and module structures.
- **Documentation**: Add doc comments (`///`) to all public items (structs, enums, functions, traits, modules).
- **Testing**: Add unit tests or integration tests for all new functionality or bug fixes.
- **Clean Commits**: Keep pull requests focused and maintain clean commit histories.

---

## License

By contributing to **heisensim**, you agree that your contributions will be dual-licensed under the terms of both the **Apache License (Version 2.0)** and the **MIT License**.

See [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT) for details.
