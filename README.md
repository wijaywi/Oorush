# Oorush

**A Post-Quantum blBlockchain framework in Rust where every wallet address has its own piano melody.**

**Oorush** is an educational blockchain framework built around Dilithium5 post-quantum signatures. Its distinctive feature is the address melody: give Oorush any public key or address and it deterministically composes a short piano piece as a WAV file. The same input always produces the same tune, so every wallet receives a recognizable sound signature.

Underneath, Oorush is a Dilithium5-signed UTXO blockchain with a small proof-of-authority (PoA) TCP node, an encrypted wallet, and read-only dashboards, designed for learning and research.

> Oorush is an educational MVP. It is not production cryptocurrency software and must not be used with real funds. See [Security and MVP Limitations](#security-and-mvp-limitations).

## Table of Contents

- [Highlights](#highlights)
- [Hear Your Address](#hear-your-address)
- [Quick Start](#quick-start)
- [Docker](#docker)
- [Wallet](#wallet)
- [Node Interfaces](#node-interfaces)
- [Dashboards](#dashboards)
- [Security and MVP Limitations](#security-and-mvp-limitations)
- [Legal and Disclaimer](#legal-and-disclaimer)

## Highlights

- **Address-to-piano melody.** Turn any public key or address into a deterministic piano melody (mono WAV). The same seed always produces the same tune.
- **Post-quantum signatures.** Transactions and blocks are signed with Dilithium5.
- **UTXO model with proof of authority.** A compact and readable chain design that is easy to study.
- **Encrypted wallets.** Argon2id key derivation with AES-256-GCM encryption. Wallet files are never overwritten automatically.
- **Read-only dashboards.** A cross-platform validator dashboard (`oorush-miner`) and a responsive network dashboard for Netlify.
- **Reproducible builds.** Pinned Rust toolchain, committed `Cargo.lock`, and an offline test suite that is also verified in Docker through GitHub Actions.

## Hear Your Address

The original deterministic piano artifact is available through the `demo` binary. It derives a mono WAV melody from a public-key or public-address seed.

Generate the default melody:

```
cargo run --bin demo -- --output oorush_piano.wav
```

Generate a melody from a specific seed, such as a public address:

```
cargo run --bin demo -- --seed oorush-public-address --output address.wav
```

Open the resulting `.wav` file in any audio player.

> The melody is intended as a human-friendly sound signature for recognition and demonstration. It is not a security feature. Do not verify addresses by ear.

## Quick Start

### Prerequisites

- Rust, pinned to `1.85.1` in `rust-toolchain.toml`. Install it with rustup.
- Docker (optional), for the containerized workflow.
- Netlify (optional), only for hosting the network dashboard.

### Run the tests

`Cargo.lock` is committed. Run the single project test command from the repository root:

```
cargo test --locked --offline
```

The locked test suite runs in offline mode. On a clean host, the first dependency download must happen during setup or during the Docker build; the test command itself does not access the network. `make test` is also available as a local convenience wrapper around the same command.

### Create a wallet

The password is read without echo:

```
cargo run --bin oorush-wallet -- generate --output node-wallet.json
```

### Start a node

```
cargo run --bin oorush-node -- --listen 127.0.0.1:18444 \
  --http-listen 127.0.0.1:18445 \
  --state node-chain.json --wallet node-wallet.json --initial-balance 1000000
```

On first start, the node creates a deterministic genesis (timestamp zero) that allocates `--initial-balance` to the wallet address. It uses create-new semantics for that initial state file; an existing state is loaded and is not replaced by genesis. Subsequent validated changes are persisted to the state file. State writes use a synced temporary file and a `.bak` recovery copy. A malformed primary state is rejected and a validated backup is attempted.

Consensus objects use the versioned local chain domain `OORUSH::LOCALNET::v2`. State files created by earlier protocol revisions are not silently migrated.

### Configuration reference

`.env.example` documents the safe loopback and `/data` deployment settings. The current binaries use command-line arguments rather than loading a dotenv file. Copy the example only if you need a local settings reference, and never put wallet passwords or private keys in it.

## Docker

Build the image from the repository root:

```
docker build --tag oorush:local .
```

Run the tests offline after the image has been built:

```
docker run --rm --network none oorush:local cargo test --locked --offline
```

The final image intentionally retains the pinned Rust toolchain and Cargo cache so this offline test can compile and run without contacting a registry.

GitHub Actions verifies the same clean-Linux flow in `.github/workflows/linux-docker.yml`. It builds the image on Ubuntu and runs the tests with `--network none`. The workflow result is the authoritative Docker verification when Docker is unavailable on the development machine.

### Generate a wallet in Docker

Create a host directory for the encrypted wallet and chain state, then generate the wallet. The command prompts for its password without echoing it:

```
mkdir -p oorush-data
docker run --rm -it --entrypoint oorush-wallet \
  -v "$(pwd)/oorush-data:/data" oorush:local \
  generate --output /data/node-wallet.json
```

### Run the node in Docker

The image defaults to the safe loopback listener `127.0.0.1:18444`. Passing `--state` and `--wallet` explicitly documents the required files and keeps them outside the image:

```
docker run --rm -it --name oorush-node \
  -p 127.0.0.1:18444:18444 \
  -v "$(pwd)/oorush-data:/data" \
  oorush:local \
  --listen 127.0.0.1:18444 \
  --state /data/node-chain.json \
  --wallet /data/node-wallet.json \
  --initial-balance 1000000
```

The wallet password is requested interactively when the container starts. Keep `oorush-data/` out of version control: runtime wallet, state, passwords, and other secrets must never be committed or copied into the image.

### Push to a registry

For a registry deployment, tag and push the image using your registry name, then run it with the same persistent `/data` volume and loopback-safe arguments:

```
docker tag oorush:local registry.example/oorush:0.1.0
docker push registry.example/oorush:0.1.0
```

The node remains loopback-only by design. Do not publish it to a LAN or the public internet without implementing and reviewing transport authentication.

## Wallet

The wallet is Oorush-native and uses Dilithium5. It is not Bitcoin or Ethereum compatible. Wallet files use Argon2id and AES-256-GCM and are never overwritten automatically:

```
cargo run --bin oorush-wallet -- generate --output node-wallet.json
```

Private-key output is deliberately opt-in and should not be copied into logs:

```
cargo run --bin oorush-wallet -- generate --output node-wallet.json --export-private-key
```

## Node Interfaces

### TCP protocol

The protocol is newline-delimited UTF-8 JSON over TCP. Requests and responses are one JSON object per line. Requests are limited to 1 MiB and responses to 8 MiB. Connections have bounded read and write timeouts and are served by a fixed worker pool with a bounded queue. Successful responses have the form `{"ok":true,"result":...}` and failures have the form `{"ok":false,"error":"..."}`.

Supported commands:

```
{"command":"get_status"}
{"command":"get_balance","address":"oorush1..."}
{"command":"get_chain"}
{"command":"submit_transaction","transaction":{...}}
{"command":"produce_block"}
```

Example using a line-oriented TCP client:

```
printf '{"command":"get_status"}\n' | nc 127.0.0.1 18444
printf '{"command":"get_balance","address":"oorush1..."}\n' | nc 127.0.0.1 18444
```

`submit_transaction` validates the signature, ownership, inputs, and pending double-spends before persistence. `produce_block` signs and validates a block with the configured wallet, then persists it. With `--peer HOST:PORT`, a locally produced block is sent to that peer as the internal `submit_block` request, and the peer validates it before applying it. This simple broadcast is not consensus or fork-choice safety.

Transactions and blocks use the JSON structures in `src/blockchain.rs` and must carry valid Dilithium5 signatures. The node serializes access to chain state with a mutex and returns explicit errors for malformed or invalid requests.

The node protocol has no transport authentication. The node therefore accepts only loopback listen addresses (`127.0.0.1`, `::1`, or another loopback IP) by default. Do not expose it on a LAN or the public internet. Adding remote access requires implementing and reviewing transport authentication first.

### Read-only node status API

Pass `--http-listen` to enable the real read-only HTTP API. It reads the running node's current chain state and validator wallet metadata. It does not accept transactions, private keys, or signing commands:

```
curl http://127.0.0.1:18445/health
curl http://127.0.0.1:18445/status
curl http://127.0.0.1:18445/node/info
curl http://127.0.0.1:18445/block/latest
```

`/status` returns live fields such as `height`, `pending`, `validators`, `utxos`, `balance`, `walletAddress`, `walletPublicKey`, `walletState`, and `latestHash`.

The HTTP API is loopback-only by design. Netlify cannot reach `127.0.0.1`. To connect the dashboard, place an authenticated HTTPS read-only gateway or tunnel in front of this API and set `OORUSH_STATUS_URL`. Do not expose the unauthenticated API directly to the public internet.

## Dashboards

### PoA validator dashboard (`oorush-miner`)

The `oorush-miner` binary is a small cross-platform GUI for research and education. Despite its historical name, it is not a proof-of-work miner. It performs no CPU mining, hash grinding, block production, transaction submission, or private-key handling. Oorush uses proof of authority, so the dashboard only observes a validator node through its read-only HTTP API.

With a node running as described in [Quick Start](#quick-start), launch the dashboard from another terminal:

```
cargo run --release --bin oorush-miner
```

The status URL defaults to `http://127.0.0.1:18445`, the node TCP address is shown as `127.0.0.1:18444`, and the wallet path is shown as `oorush-wallet.json`. Edit the status URL or TCP address if your local node uses different ports, then choose **Refresh status**.

The dashboard performs only a local `GET /status`. It accepts loopback HTTP URLs (`localhost`, `127.0.0.1`, or `::1`) and rejects external hosts and HTTPS. It displays live public chain and validator metadata, never passwords or private keys.

Build the Windows GUI executable with:

```
cargo build --release --bin oorush-miner
```

On Windows the output is `target\release\oorush-miner.exe`. The same source can be checked on Linux with Cargo. A native Windows build or Windows GUI run is still required to verify the platform-specific executable and windowing integration.

### Netlify network dashboard

`public/index.html` is a responsive network dashboard. It shows a clearly labelled demo stream until a live HTTP status source is configured.

Netlify cannot run the Rust node or connect to its loopback-only TCP socket. To show real data, deploy a separate authenticated read-only HTTP gateway next to the nodes and set the Netlify environment variable `OORUSH_STATUS_URL` to its HTTPS URL. The included `netlify/functions/network-status.js` proxies that document at `/api/network-status`. It never receives wallet keys or signs transactions.

The gateway should return JSON with these optional fields:

```
{"height":12842,"healthyNodes":18,"transactionsPerHour":1284,"blockTime":"18.6s","walletAddress":"oorush1...","walletPublicKey":"dilithium5:...","walletState":"encrypted"}
```

For a direct test endpoint, use `?api=https://gateway.example/status` in the dashboard URL. Do not expose the unauthenticated TCP node publicly. The dashboard is observational only. Mining and transaction submission still occur on the Rust nodes.

`netlify.toml` configures Netlify to publish the dashboard and its function. Use these settings when connecting the repository:

```
Base directory:      (empty)
Build command:       (empty)
Publish directory:   public
Functions directory: netlify/functions
```

Netlify hosts the dashboard and proxy only. The Rust blockchain node still requires Docker or another long-running server environment and is not started by Netlify.

## Security and MVP Limitations

This is an educational PoA blockchain, not production cryptocurrency software. The current implementation provides UTXO balances, Dilithium transaction and block signatures, encrypted local wallet storage, persisted chain state, and a small TCP JSON node.

It does not yet provide:

- Byzantine fault tolerance
- Secure peer authentication and encryption
- Peer discovery
- Fork choice
- Chain synchronization
- Finality
- Validator rotation
- Fees
- Mempool gossip
- Replay protection across networks
- Audited cryptographic or economic security

Do not use it for real funds before independent review and a threat-model-driven redesign. See `SECURITY.md` for the security policy.

## Legal and Disclaimer

**IMPORTANT NOTICE:** This software is provided purely as an experimental, open-source research module.

**Zero liability:** As stated in the `LICENSE` (MIT License), this software is provided "AS IS". The creators, authors, and contributors accept zero liability for hardware damage, or legal repercussions resulting from compiling, modifying, or integrating this code.

**Unaudited code:** This module is educational and has not undergone formal security audits.

## License

Released under the MIT License. See `LICENSE` for details.
