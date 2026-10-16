FROM rust:1.85.1-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked --bin oorush-node --bin oorush-wallet

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install --no-install-recommends --yes ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/oorush-node /usr/local/bin/oorush-node
COPY --from=builder /build/target/release/oorush-wallet /usr/local/bin/oorush-wallet

WORKDIR /data
VOLUME ["/data"]

# The node is intentionally loopback-only. Supply a wallet and state in /data.
EXPOSE 18444
ENTRYPOINT ["oorush-node"]
CMD ["--listen", "127.0.0.1:18444", "--state", "/data/node-chain.json", "--wallet", "/data/node-wallet.json", "--initial-balance", "1000000"]
