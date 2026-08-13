#!/usr/bin/env bash
set -euo pipefail

K3D_VERSION="v5.9.0"

echo "Installing k3d ${K3D_VERSION}..."
curl --fail --location --silent --max-time 60 \
  "https://github.com/k3d-io/k3d/releases/download/${K3D_VERSION}/k3d-linux-amd64" \
  -o /tmp/k3d
chmod +x /tmp/k3d
sudo mv /tmp/k3d /usr/local/bin/k3d
k3d --version

echo "Building heisensim..."
cargo build --release -p heisensim
sudo cp target/release/heisensim /usr/local/bin/

echo ""
echo "✅ Ready! Run: heisensim demo"
