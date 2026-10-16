# PQC (Post-Quantum Cryptography) - Oorush- PoC

This document describes the post-quantum cryptography used by the Oorush blockchain MVP.

## Goals
- Provide a PoC for post-quantum signatures (Dilithium) and key exchange (Kyber).
- Demonstrate a hybrid approach: classic (secp256k1) + PQC (Dilithium).
- Use Dilithium5 signatures for wallet identities, transactions, and PoA blocks.
- Use Kyber512 for the standalone key-exchange demonstration.

## Related Files
- `src/pqc/dilithium_precompile.rs` - Dilithium signing and verification.
- `src/blockchain.rs` - signed UTXO transactions and PoA blocks.
- `src/wallet.rs` - encrypted wallet storage and address derivation.
- `README.md` - build and node usage.

## ⚙️ Example configuration (draft)
```yaml
pqc:
enabled: false
pqc_default_alg: "dilithium2"
pqc_kem_for_p2p: "kyber512"
allow_hybrid_tx: true
