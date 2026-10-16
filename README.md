# Oorush blockchain MVP

This repository contains an educational Dilithium5-signed UTXO blockchain and
a small proof-of-authority TCP node. It is intentionally an MVP: it does not
provide Byzantine fault tolerance, fork choice, finality, peer discovery,
transport security, or production authentication.

## Reproducible setup

The Rust toolchain is pinned to `1.85.1` in `rust-toolchain.toml`, and
`Cargo.lock` is committed. Install Rust with rustup, then run the single
project test command from the repository root:

```text
cargo test --locked --offline
```

The locked Cargo test suite runs in offline mode. On a clean host, the first
dependency download must happen during setup or Docker build; the test command
itself does not access the network. `make test` is also available as a local
convenience wrapper around the same command.

`.env.example` documents the safe loopback and `/data` deployment settings.
The current binaries use command-line arguments rather than loading a dotenv
file; copy the example only if you need a local settings reference, and never
put wallet passwords or private keys in it.

## Netlify network dashboard

`public/index.html` is a responsive network dashboard. It shows a clearly
labelled demo stream until a live HTTP status source is configured. Netlify
cannot run the Rust node or connect to its loopback-only TCP socket. To show
real data, deploy a separate authenticated read-only HTTP gateway next to the
nodes and set the Netlify environment variable `OORUSH_STATUS_URL` to its
HTTPS URL. The included `netlify/functions/network-status.js` proxies that
document at `/api/network-status`; it never receives wallet keys or signs
transactions.

The gateway should return JSON with these optional fields:

```json
{"height":12842,"healthyNodes":18,"transactionsPerHour":1284,"blockTime":"18.6s"}
```

For a direct test endpoint, use `?api=https://gateway.example/status` in the
dashboard URL. Do not expose the unauthenticated TCP node publicly. The
dashboard is observational only; mining and transaction submission still occur
on the Rust nodes.

`netlify.toml` configures Netlify to publish the dashboard and its function, so
use these settings when connecting the repository:

```text
Base directory:      (empty)
Build command:       (empty)
Publish directory:   public
Functions directory: netlify/functions
```

Netlify hosts the dashboard and proxy only. The Rust blockchain node still
requires Docker or another long-running server environment and is not started
by Netlify.

The node protocol has no transport authentication. The node therefore accepts
only loopback listen addresses (`127.0.0.1`, `::1`, or another loopback IP) by
default; do not expose it on a LAN or the public internet. Adding remote access
requires implementing and reviewing transport authentication first.

## Start a node

### Docker

Build the image from the repository root:

```text
docker build --tag oorush:local .
```

Judge-style offline test after the image has been built:

```text
docker run --rm --network none oorush:local cargo test --locked --offline
```

The final image intentionally retains the pinned Rust toolchain and Cargo
cache so this offline test can compile and run without contacting a registry.

Create a host directory for the encrypted wallet and chain state, then generate
the wallet. The wallet-generation command prompts for its password without
echoing it:

```text
mkdir -p oorush-data
docker run --rm -it --entrypoint oorush-wallet \
  -v "$(pwd)/oorush-data:/data" oorush:local \
  generate --output /data/node-wallet.json
```

Run the node with its persistent data directory mounted. The image defaults to
the safe loopback listener `127.0.0.1:18444`; explicitly passing `--state` and
`--wallet` documents the required files and keeps them outside the image:

```text
docker run --rm -it --name oorush-node \
  -p 127.0.0.1:18444:18444 \
  -v "$(pwd)/oorush-data:/data" \
  oorush:local \
  --listen 127.0.0.1:18444 \
  --state /data/node-chain.json \
  --wallet /data/node-wallet.json \
  --initial-balance 1000000
```

The wallet password is requested interactively when the container starts. Keep
`oorush-data/` out of version control: runtime wallet, state, passwords, and
other secrets must never be committed or copied into the image. For a registry
deployment, tag and push the image using your registry name, then run that
image with the same persistent `/data` volume and loopback-safe arguments:

```text
docker tag oorush:local registry.example/oorush:0.1.0
docker push registry.example/oorush:0.1.0
```

The node remains loopback-only by design. Do not publish it to a LAN or the
public internet without implementing and reviewing transport authentication.

First create an encrypted wallet (the password is read without echo):

```text
cargo run --bin oorush-wallet -- generate --output node-wallet.json
```

Then start the node:

```text
cargo run --bin oorush-node -- --listen 127.0.0.1:18444 \
  --state node-chain.json --wallet node-wallet.json --initial-balance 1000000
```

On first start, the node creates a deterministic genesis (timestamp zero)
allocating `--initial-balance` to the wallet address. It uses create-new
semantics for that initial state file; an existing state is loaded and is not
replaced by genesis. Subsequent validated changes are persisted to the state
file. State writes use a synced temporary file and a `.bak` recovery copy; a
malformed primary state is rejected and a validated backup is attempted.
Consensus objects use the versioned local chain domain `OORUSH::LOCALNET::v2`;
state files created by earlier protocol revisions are not silently migrated.

## TCP protocol

The protocol is newline-delimited UTF-8 JSON over TCP. Requests and responses
are one JSON object per line; requests are limited to 1 MiB and responses to
8 MiB. Connections have bounded read/write timeouts and are served by a fixed
worker pool with a bounded queue. Successful
responses have `{"ok":true,"result":...}` and failures have
`{"ok":false,"error":"..."}`.

```text
{"command":"get_status"}
{"command":"get_balance","address":"oorush1..."}
{"command":"get_chain"}
{"command":"submit_transaction","transaction":{...}}
{"command":"produce_block"}
```

`submit_transaction` validates the signature, ownership, inputs, and pending
double-spends before persistence. `produce_block` signs and validates a block
with the configured wallet, then persists it. With `--peer HOST:PORT`, a
locally produced block is sent to that peer as the internal
`submit_block` request; the peer validates it before applying it. This simple
broadcast is not consensus or fork-choice safety.

For example, with a line-oriented TCP client:

```text
printf '{"command":"get_status"}\n' | nc 127.0.0.1 18444
printf '{"command":"get_balance","address":"oorush1..."}\n' | nc 127.0.0.1 18444
```

Transactions and blocks use the JSON structures in `src/blockchain.rs` and
must carry valid Dilithium5 signatures. The node serializes access to chain
state with a mutex and returns explicit errors for malformed or invalid
requests.

## Wallet and piano artifact

The wallet is Oorush-native and uses Dilithium5. It is not Bitcoin or
Ethereum-compatible. Wallet files use Argon2id and AES-256-GCM and are never
overwritten automatically:

```text
cargo run --bin oorush-wallet -- generate --output node-wallet.json
```

Private-key output is deliberately opt-in and should not be copied into logs:

```text
cargo run --bin oorush-wallet -- generate --output node-wallet.json --export-private-key
```

The original deterministic piano artifact is still available through the demo
binary. It derives a mono WAV melody from a public-key or public-address seed:

```text
cargo run --bin demo -- --output oorush_piano.wav
cargo run --bin demo -- --seed oorush-public-address --output address.wav
```

## Security and MVP limitations

This is an educational PoA blockchain, not production cryptocurrency software.
The current implementation has UTXO balances, Dilithium transaction and block
signatures, encrypted local wallet storage, persisted chain state, and a small
TCP JSON node. It does **not** yet provide Byzantine fault tolerance, secure
peer authentication/encryption, peer discovery, fork choice, chain sync,
finality, validator rotation, fees, mempool gossip, replay protection across
networks, or audited cryptographic/economic security. Do not use it for real
funds before independent review and a threat-model-driven redesign.
