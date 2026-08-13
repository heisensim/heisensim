#!/usr/bin/env bash
set -euo pipefail

K3D_VERSION="v5.9.0"
K3D_SHA256="06d8f25bc3a971c4eb29e0ff08429b180402db0f4dec838c9eac427e296800a0"

echo "Installing k3d ${K3D_VERSION}..."

TMPDIR="$(mktemp -d)"
trap 'rm -rf "${TMPDIR}"' EXIT

curl --fail --location --silent --max-time 60 \
  "https://github.com/k3d-io/k3d/releases/download/${K3D_VERSION}/k3d-linux-amd64" \
  -o "${TMPDIR}/k3d"

echo "${K3D_SHA256}  ${TMPDIR}/k3d" | sha256sum --check --quiet
sudo install -m 0755 "${TMPDIR}/k3d" /usr/local/bin/k3d
k3d --version

echo "Building heisensim..."
cargo build --release -p heisensim
sudo install -m 0755 target/release/heisensim /usr/local/bin/heisensim

echo ""
echo "✅ Ready! Run: heisensim demo"
