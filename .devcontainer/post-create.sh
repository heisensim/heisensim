#!/usr/bin/env bash
set -e

echo "Installing k3d..."
curl -s https://raw.githubusercontent.com/k3d-io/k3d/main/install.sh | bash

echo "Building heisensim..."
cargo build --release -p heisensim
sudo cp target/release/heisensim /usr/local/bin/

echo "Ready! Run: heisensim demo"
