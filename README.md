# Oorush blockchain MVP

This repository contains an educational Dilithium5-signed UTXO blockchain and
a small proof-of-authority TCP node. It is intentionally an MVP: it does not
provide Byzantine fault tolerance, fork choice, finality, peer discovery,
transport security, or production authentication.

## Start a node

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
file.

## TCP protocol

The protocol is newline-delimited UTF-8 JSON over TCP. Requests and responses
are one JSON object per line; requests are limited to 1 MiB. Successful
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
