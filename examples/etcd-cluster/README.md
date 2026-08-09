# etcd 3-Node Cluster Simulation Example

This example demonstrates how to use `heisensim` to perform fault-injection testing on a 3-node [`etcd`](https://etcd.io/) cluster under virtualized deterministic execution.

## Overview

The simulation configures three `etcd` nodes (`etcd-1`, `etcd-2`, `etcd-3`) running version `v3.5.9`. `heisensim` injects controlled faults while verifying invariant properties throughout the simulation run:

- **Fault Injections**:
  - **Network Partitions** (10% probability): Simulates intermittent network isolation between cluster members.
  - **Process Crashes** (5% probability): Simulates unexpected process terminations and node restarts.
  - **Clock Skew** (up to 500 ms): Introduces virtual clock drifts to test Raft leader election timeouts.

- **Invariant Properties Verified**:
  - `no_crash`: Ensures processes recover cleanly without unhandled crashes.
  - `no_hang`: Confirms the Raft cluster makes forward progress within 30 virtual seconds.
  - `linearizable_reads`: Validates read linearizability guarantees across leader transitions.

## Running the Simulation

Run the simulation with the following command:

```bash
heisensim run --config examples/etcd-cluster/heisensim.toml
```

To reproduce a specific failure scenario, pass the seed output from a previous run:

```bash
heisensim run --config examples/etcd-cluster/heisensim.toml --seed 42
```
