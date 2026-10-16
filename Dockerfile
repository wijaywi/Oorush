# Keep the Rust toolchain and Cargo cache in the final image so judges can run
# `cargo test --locked --offline` after the network is disabled.
FROM rust:1.85.1-bookworm

WORKDIR /app
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src ./src
COPY tests ./tests
RUN cargo build --release --locked \
    --bin oorush-node --bin oorush-wallet --bin demo

ENV PATH="/app/target/release:${PATH}"

# Keep the project directory as the default so `docker run ... cargo test`
# finds Cargo.toml while the node stores runtime data under /data.
WORKDIR /app
VOLUME ["/data"]

# The node is intentionally loopback-only. Supply a wallet and state in /data.
EXPOSE 18444
CMD ["oorush-node", "--listen", "127.0.0.1:18444", "--state", "/data/node-chain.json", "--wallet", "/data/node-wallet.json", "--initial-balance", "1000000"]
