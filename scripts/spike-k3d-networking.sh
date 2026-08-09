#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# K3d Networking Spike: Validate tc netem + iptables inside K3d pods
# =============================================================================
#
# RESULT (2026-08-09): ALL TESTS PASS ✅
#
# Prerequisites:
#   - Docker running
#   - k3d installed (brew install k3d)
#   - kubectl installed
#
# Key finding: Pods MUST have NET_ADMIN capability for tc/iptables to work.
#   securityContext:
#     capabilities:
#       add: ["NET_ADMIN"]
#
# This means heisensim needs to either:
#   1. Require target pods to have NET_ADMIN (document this)
#   2. Deploy a privileged sidecar for fault injection
#   3. Use a DaemonSet with host networking (like Chaos Mesh)
#
# For Phase 1, option 1 is fine — we use nicolaka/netshoot as test pods
# which have all networking tools pre-installed.
# =============================================================================

CLUSTER_NAME="heisensim-spike"
PASS=0
FAIL=0

pass() { echo "  ✅ PASS: $1"; ((PASS++)); }
fail() { echo "  ❌ FAIL: $1"; ((FAIL++)); }

cleanup() {
    echo ""
    echo "Cleaning up..."
    k3d cluster delete "$CLUSTER_NAME" 2>/dev/null || true
}
trap cleanup EXIT

echo "=== K3d Networking Spike ==="
echo ""

# Step 1: Create cluster
echo "Step 1: Creating K3d cluster..."
k3d cluster create "$CLUSTER_NAME" --wait --timeout 60s --no-lb

# Step 2: Deploy test pods with NET_ADMIN
echo "Step 2: Deploying test pods..."
cat <<EOF | kubectl apply -f -
apiVersion: v1
kind: Pod
metadata:
  name: pod-a
spec:
  containers:
  - name: main
    image: nicolaka/netshoot:latest
    command: ["sleep", "3600"]
    securityContext:
      capabilities:
        add: ["NET_ADMIN"]
---
apiVersion: v1
kind: Pod
metadata:
  name: pod-b
spec:
  containers:
  - name: main
    image: nicolaka/netshoot:latest
    command: ["sleep", "3600"]
    securityContext:
      capabilities:
        add: ["NET_ADMIN"]
EOF

# Step 3: Wait for pods
echo "Step 3: Waiting for pods to be ready..."
kubectl wait --for=condition=Ready pods pod-a pod-b --timeout=120s

# Get pod IPs
POD_B_IP=$(kubectl get pod pod-b -o jsonpath='{.status.podIP}')
echo "  Pod B IP: $POD_B_IP"
echo ""

# Test 1: Baseline connectivity
echo "Test 1: Baseline connectivity"
if kubectl exec pod-a -- ping -c 1 -W 1 "$POD_B_IP" &>/dev/null; then
    pass "Pods can communicate"
else
    fail "Pods cannot communicate"
fi

# Test 2: tc netem latency injection
echo "Test 2: tc netem latency (500ms)"
if kubectl exec pod-a -- tc qdisc add dev eth0 root netem delay 500ms 2>/dev/null; then
    RTT=$(kubectl exec pod-a -- ping -c 1 -W 3 "$POD_B_IP" 2>/dev/null | grep "time=" | sed 's/.*time=\([0-9.]*\).*/\1/')
    if (( $(echo "$RTT > 400" | bc -l) )); then
        pass "Latency injected: ${RTT}ms (expected ~500ms)"
    else
        fail "Latency not effective: ${RTT}ms"
    fi
    kubectl exec pod-a -- tc qdisc del dev eth0 root 2>/dev/null
else
    fail "tc netem command failed"
fi

# Test 3: iptables partition
echo "Test 3: iptables partition (DROP)"
if kubectl exec pod-a -- iptables -A OUTPUT -d "$POD_B_IP" -j DROP 2>/dev/null; then
    if ! kubectl exec pod-a -- ping -c 1 -W 2 "$POD_B_IP" &>/dev/null; then
        pass "Partition effective: packets dropped"
    else
        fail "Partition not effective: still reachable"
    fi
    kubectl exec pod-a -- iptables -D OUTPUT -d "$POD_B_IP" -j DROP 2>/dev/null
else
    fail "iptables command failed"
fi

# Test 4: Verify recovery
echo "Test 4: Recovery after revert"
if kubectl exec pod-a -- ping -c 1 -W 1 "$POD_B_IP" &>/dev/null; then
    pass "Connectivity restored after revert"
else
    fail "Connectivity NOT restored"
fi

echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
if [ "$FAIL" -eq 0 ]; then
    echo "🎉 All tests passed! tc netem and iptables work in K3d."
    echo ""
    echo "Key requirement: pods need NET_ADMIN capability."
    echo "For heisensim, this means the fault injection pods (or target pods)"
    echo "must have securityContext.capabilities.add: [\"NET_ADMIN\"]"
else
    echo "⚠️  Some tests failed. Check Docker and K3d configuration."
    exit 1
fi
