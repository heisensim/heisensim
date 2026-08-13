# heisensim GitHub Action

## Quick Start

```yaml
name: Chaos Test
on: [push]
jobs:
  chaos:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: heisensim/heisensim@v0.4.0
        with:
          config: heisensim.toml
```

## Inputs

| Input | Description | Required | Default |
|:------|:-----------|:---------|:--------|
| `config` | Path to config file | Yes | - |
| `version` | heisensim version | No | `latest` |
| `args` | Extra CLI arguments | No | - |
| `upload-results` | Upload JUnit artifact | No | `true` |
