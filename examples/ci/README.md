# Heisensim CI Examples

This directory contains example configurations for integrating Heisensim into your Continuous Integration (CI) pipelines.

## What these configs do

These configs demonstrate how to run deterministic chaos testing in CI without needing a real Kubernetes cluster:
1. They install Heisensim (or use a Docker image).
2. They run a specific simulation with a fixed seed (`heisensim simulate --seed 0x42`).
3. They output the results in JUnit format for test reporting.
4. They run exploration across many seeds to find edge cases (`heisensim explore`).

## How to customize

You can customize these examples by modifying the following flags:
- `--seed`: Change the random seed for reproducible chaos.
- `--duration`: Adjust the simulated time length.
- `--config`: Point to your specific `heisensim.toml` configuration file.

For more details, see the [Heisensim documentation](https://github.com/heisensim/heisensim).
