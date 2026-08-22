# Heisensim CI Examples

Add deterministic chaos testing to your CI pipeline — no cluster required.

## GitHub Actions

### Option A: Official Action (recommended)

```yaml
- uses: heisensim/action@v1
  with:
    config: heisensim.toml
    seeds: '50'
    bisect: 'true'
```

See [heisensim/action](https://github.com/heisensim/action) for full docs.

### Option B: Direct installation

See [`github-actions.yml`](github-actions.yml) for a copy-paste workflow.

## GitLab CI

### Option A: Include template (recommended)

```yaml
include:
  - remote: 'https://raw.githubusercontent.com/heisensim/heisensim/main/examples/ci/gitlab-template.yml'

chaos-test:
  extends: .heisensim-chaos-test
  variables:
    HEISENSIM_CONFIG: heisensim.toml
    HEISENSIM_SEEDS: "50"
    HEISENSIM_BISECT: "true"
```

See [`gitlab-template.yml`](gitlab-template.yml) for all available variables.

### Option B: Copy-paste

See [`gitlab-ci.yml`](gitlab-ci.yml) for a minimal standalone config.

## Getting started

1. Generate a config: `heisensim init --preset ci` (or `--preset microservice`)
2. Edit `heisensim.toml` with your service probes and SLA properties
3. Add the CI config above to your pipeline
4. Commit and push — chaos testing runs on every PR
