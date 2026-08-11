# Stage 1: Build heisensim
FROM rust:1.87-bookworm AS builder

WORKDIR /build
COPY . .
RUN cargo build --release --bin heisensim \
    && strip target/release/heisensim

# Stage 2: Minimal runtime
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        iproute2 \
        iptables \
    && curl -fsSL "https://dl.k8s.io/release/$(curl -fsSL https://dl.k8s.io/release/stable.txt)/bin/linux/amd64/kubectl" \
        -o /usr/local/bin/kubectl \
    && chmod +x /usr/local/bin/kubectl \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/heisensim /usr/local/bin/heisensim

ENTRYPOINT ["heisensim"]
