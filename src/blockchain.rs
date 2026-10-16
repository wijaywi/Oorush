//! Educational proof-of-authority blockchain MVP.
//!
//! This module intentionally favors a small, explicit, auditable design over
//! production consensus features. It is not production consensus: there is no
//! networking, fork choice, finality gadget, fee market, or key management.

use anyhow::{anyhow, bail, Context, Result};
use pqcrypto_dilithium::dilithium5::{
    detached_sign, verify_detached_signature, DetachedSignature as DilithiumSignature, PublicKey,
    SecretKey,
};
use pqcrypto_traits::sign::{
    DetachedSignature, PublicKey as PublicKeyTrait, SecretKey as SecretKeyTrait,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub type Hash = [u8; 32];
pub type Address = String;

const MAX_STATE_BYTES: usize = 16 * 1024 * 1024;
const MAX_BLOCKS: usize = 100_000;
const MAX_BLOCK_TRANSACTIONS: usize = 10_000;
const MAX_PENDING_TRANSACTIONS: usize = 10_000;
const MAX_TX_INPUTS: usize = 1_000;
const MAX_TX_OUTPUTS: usize = 1_000;
const MAX_TX_BYTES: usize = 512 * 1024;

type PersistedState = (
    Vec<Block>,
    Vec<(OutPoint, TxOutput)>,
    BTreeMap<Address, Vec<u8>>,
    Vec<Transaction>,
);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct OutPoint {
    pub txid: Hash,
    pub index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TxInput {
    pub previous_output: OutPoint,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TxOutput {
    pub address: Address,
    /// Amount in the chain's smallest integer unit.
    pub amount: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transaction {
    pub inputs: Vec<TxInput>,
    pub outputs: Vec<TxOutput>,
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
}

impl Transaction {
    pub fn unsigned(inputs: Vec<TxInput>, outputs: Vec<TxOutput>, public_key: Vec<u8>) -> Self {
        Self {
            inputs,
            outputs,
            public_key,
            signature: Vec::new(),
        }
    }

    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(&(
            "OORUSH::TX::DILITHIUM5::v1",
            &self.inputs,
            &self.outputs,
            &self.public_key,
        ))
        .context("serialize transaction signing payload")
    }

    pub fn sign(&mut self, secret_key: &[u8]) -> Result<()> {
        let secret = SecretKey::from_bytes(secret_key)
            .map_err(|_| anyhow!("invalid Dilithium5 secret key"))?;
        self.signature = detached_sign(&self.signing_bytes()?, &secret)
            .as_bytes()
            .to_vec();
        Ok(())
    }

    pub fn txid(&self) -> Result<Hash> {
        Ok(hash_bytes(&serde_json::to_vec(self)?))
    }

    pub fn verify_signature(&self) -> Result<bool> {
        let public = PublicKey::from_bytes(&self.public_key)
            .map_err(|_| anyhow!("invalid Dilithium5 public key"))?;
        let signature = DilithiumSignature::from_bytes(&self.signature)
            .map_err(|_| anyhow!("invalid transaction signature"))?;
        Ok(verify_detached_signature(&signature, &self.signing_bytes()?, &public).is_ok())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Block {
    pub index: u64,
    pub previous_hash: Hash,
    pub timestamp: u64,
    pub transactions: Vec<Transaction>,
    pub validator_public_key: Vec<u8>,
    pub validator_signature: Vec<u8>,
}

impl Block {
    fn unsigned_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(&(
            "OORUSH::BLOCK::DILITHIUM5::v1",
            self.index,
            self.previous_hash,
            self.timestamp,
            &self.transactions,
            &self.validator_public_key,
        ))
        .context("serialize block signing payload")
    }
    pub fn hash(&self) -> Result<Hash> {
        Ok(hash_bytes(&serde_json::to_vec(self)?))
    }
    pub fn verify_validator_signature(&self) -> Result<bool> {
        let public = PublicKey::from_bytes(&self.validator_public_key)
            .map_err(|_| anyhow!("invalid validator public key"))?;
        let sig = DilithiumSignature::from_bytes(&self.validator_signature)
            .map_err(|_| anyhow!("invalid block signature"))?;
        Ok(verify_detached_signature(&sig, &self.unsigned_bytes()?, &public).is_ok())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainState {
    pub blocks: Vec<Block>,
    pub utxos: BTreeMap<OutPoint, TxOutput>,
    pub validators: BTreeMap<Address, Vec<u8>>,
    pub pending_transactions: Vec<Transaction>,
}

impl ChainState {
    /// Build a deterministic genesis block from initial balances and validator keys.
    pub fn create_genesis(
        allocations: Vec<TxOutput>,
        validator_public_keys: Vec<Vec<u8>>,
        timestamp: u64,
    ) -> Result<Self> {
        if allocations.is_empty() || allocations.len() > MAX_TX_OUTPUTS {
            bail!("invalid genesis allocations")
        }
        let validator_count = validator_public_keys.len();
        let mut validators = BTreeMap::new();
        for key in validator_public_keys {
            let public =
                PublicKey::from_bytes(&key).map_err(|_| anyhow!("invalid validator public key"))?;
            validators.insert(
                crate::wallet::address_from_public_key(public.as_bytes()),
                key,
            );
        }
        if validators.len() != validator_count {
            bail!("duplicate validator public key")
        }
        if validators.is_empty() {
            bail!("genesis requires at least one validator")
        }
        let genesis_tx = Transaction::unsigned(Vec::new(), allocations, Vec::new());
        let _genesis_supply = genesis_tx.outputs.iter().try_fold(0_u64, |sum, output| {
            validate_output(output)?;
            sum
                .checked_add(output.amount)
                .ok_or_else(|| anyhow!("genesis supply overflow"))
        })?;
        let txid = genesis_tx.txid()?;
        let block = Block {
            index: 0,
            previous_hash: [0; 32],
            timestamp,
            transactions: vec![genesis_tx],
            validator_public_key: Vec::new(),
            validator_signature: Vec::new(),
        };
        let mut utxos = BTreeMap::new();
        let outputs = &block.transactions[0].outputs;
        for (index, output) in outputs.iter().enumerate() {
            utxos.insert(
                OutPoint {
                    txid,
                    index: index as u32,
                },
                output.clone(),
            );
        }
        Ok(Self {
            blocks: vec![block],
            utxos,
            validators,
            pending_transactions: Vec::new(),
        })
    }

    pub fn submit_transaction(&mut self, transaction: Transaction) -> Result<Hash> {
        if self.pending_transactions.len() >= MAX_PENDING_TRANSACTIONS {
            bail!("pending transaction limit exceeded")
        }
        let pending_spent: BTreeSet<OutPoint> = self
            .pending_transactions
            .iter()
            .flat_map(|tx| tx.inputs.iter().map(|input| input.previous_output.clone()))
            .collect();
        self.validate_transaction(&transaction, &self.utxos, &pending_spent)?;
        let txid = transaction.txid()?;
        self.pending_transactions.push(transaction);
        Ok(txid)
    }

    pub fn produce_block(
        &self,
        transactions: Vec<Transaction>,
        validator_secret_key: &[u8],
        timestamp: u64,
    ) -> Result<Block> {
        let secret = SecretKey::from_bytes(validator_secret_key)
            .map_err(|_| anyhow!("invalid validator secret key"))?;
        // Identify the registered public key by a short challenge. The secret key
        // never leaves this function and is never formatted or logged.
        let challenge = b"OORUSH::POA::AUTH::v1";
        let proof = detached_sign(challenge, &secret);
        let validator_public_key = self
            .validators
            .values()
            .find(|key| {
                PublicKey::from_bytes(key)
                    .ok()
                    .map(|public| verify_detached_signature(&proof, challenge, &public).is_ok())
                    .unwrap_or(false)
            })
            .cloned()
            .ok_or_else(|| anyhow!("unauthorized validator"))?;
        self.produce_block_with_public_key(
            transactions,
            &validator_public_key,
            validator_secret_key,
            timestamp,
        )
    }

    pub fn produce_block_with_public_key(
        &self,
        transactions: Vec<Transaction>,
        validator_public_key: &[u8],
        validator_secret_key: &[u8],
        timestamp: u64,
    ) -> Result<Block> {
        if transactions.len() > MAX_BLOCK_TRANSACTIONS {
            bail!("too many transactions")
        }
        if timestamp < self.blocks.last().map(|b| b.timestamp).unwrap_or(0) {
            bail!("block timestamp rollback")
        }
        if !self
            .validators
            .values()
            .any(|key| key.as_slice() == validator_public_key)
        {
            bail!("unauthorized validator")
        }
        let secret = SecretKey::from_bytes(validator_secret_key)
            .map_err(|_| anyhow!("invalid validator secret key"))?;
        let public = PublicKey::from_bytes(validator_public_key)
            .map_err(|_| anyhow!("invalid validator public key"))?;
        let mut block = Block {
            index: self.blocks.len() as u64,
            previous_hash: self.blocks.last().unwrap().hash()?,
            timestamp,
            transactions,
            validator_public_key: public.as_bytes().to_vec(),
            validator_signature: Vec::new(),
        };
        block.validator_signature = detached_sign(&block.unsigned_bytes()?, &secret)
            .as_bytes()
            .to_vec();
        Ok(block)
    }

    pub fn validate_and_append_block(&mut self, block: Block) -> Result<()> {
        if self.blocks.len() >= MAX_BLOCKS || block.transactions.len() > MAX_BLOCK_TRANSACTIONS {
            bail!("chain or block size limit exceeded")
        }
        if serde_json::to_vec(&block)?.len() > MAX_STATE_BYTES {
            bail!("block too large")
        }
        if block.index != self.blocks.len() as u64 {
            bail!("unexpected block index")
        }
        if block.previous_hash != self.blocks.last().unwrap().hash()? {
            bail!("invalid previous block hash")
        }
        if block.timestamp < self.blocks.last().unwrap().timestamp {
            bail!("block timestamp rollback")
        }
        if !self
            .validators
            .values()
            .any(|key| key == &block.validator_public_key)
        {
            bail!("unauthorized validator")
        }
        if !block.verify_validator_signature()? {
            bail!("invalid validator signature")
        }
        let mut next = self.utxos.clone();
        let mut spent = BTreeSet::new();
        for tx in &block.transactions {
            self.validate_transaction(tx, &next, &spent)?;
            apply_transaction(tx, &mut next, &mut spent)?;
        }
        self.utxos = next;
        self.pending_transactions
            .retain(|pending| !block.transactions.iter().any(|tx| tx == pending));
        self.blocks.push(block);
        Ok(())
    }

    pub fn balance(&self, address: &str) -> Result<u64> {
        self.utxos
            .values()
            .filter(|o| o.address == address)
            .try_fold(0u64, |sum, o| {
                sum.checked_add(o.amount)
                    .ok_or_else(|| anyhow!("balance overflow"))
            })
    }
    pub fn utxos_for(&self, address: &str) -> Vec<(OutPoint, TxOutput)> {
        self.utxos
            .iter()
            .filter(|(_, o)| o.address == address)
            .map(|(p, o)| (p.clone(), o.clone()))
            .collect()
    }
    pub fn serialize(&self) -> Result<Vec<u8>> {
        // JSON objects require string keys, so encode the ordered UTXO map as
        // pairs while retaining deterministic ordering and round-trip safety.
        Ok(serde_json::to_vec(&(
            &self.blocks,
            self.utxos.iter().collect::<Vec<_>>(),
            &self.validators,
            &self.pending_transactions,
        ))?)
    }
    pub fn deserialize(bytes: &[u8]) -> Result<Self> {
        if bytes.is_empty() || bytes.len() > MAX_STATE_BYTES {
            bail!("invalid or oversized chain state")
        }
        let (blocks, utxos, validators, pending_transactions): PersistedState =
            serde_json::from_slice(bytes)?;
        if blocks.is_empty()
            || blocks.len() > MAX_BLOCKS
            || pending_transactions.len() > MAX_PENDING_TRANSACTIONS
        {
            bail!("invalid chain state size")
        }
        let mut state = Self {
            blocks,
            utxos: BTreeMap::new(),
            validators,
            pending_transactions,
        };
        for (point, output) in utxos {
            if state.utxos.insert(point, output).is_some() {
                bail!("duplicate serialized UTXO")
            }
        }
        state.validate_persisted_state()?;
        Ok(state)
    }

    fn validate_transaction(
        &self,
        tx: &Transaction,
        utxos: &BTreeMap<OutPoint, TxOutput>,
        spent: &BTreeSet<OutPoint>,
    ) -> Result<()> {
        if tx.inputs.is_empty()
            || tx.outputs.is_empty()
            || tx.inputs.len() > MAX_TX_INPUTS
            || tx.outputs.len() > MAX_TX_OUTPUTS
        {
            bail!("transaction must have inputs and outputs")
        }
        if serde_json::to_vec(tx)?.len() > MAX_TX_BYTES {
            bail!("transaction too large")
        }
        if !tx.verify_signature()? {
            bail!("invalid transaction signature")
        }
        let sender = crate::wallet::address_from_public_key(&tx.public_key);
        let mut input_total = 0u64;
        let mut seen = BTreeSet::new();
        for input in &tx.inputs {
            if !seen.insert(input.previous_output.clone()) || spent.contains(&input.previous_output)
            {
                bail!("double-spend")
            }
            let output = utxos
                .get(&input.previous_output)
                .ok_or_else(|| anyhow!("unknown input"))?;
            if output.address != sender {
                bail!("input is not owned by signer")
            }
            input_total = input_total
                .checked_add(output.amount)
                .ok_or_else(|| anyhow!("input amount overflow"))?;
        }
        let output_total = tx.outputs.iter().try_fold(0u64, |sum, o| {
            validate_output(o)?;
            sum.checked_add(o.amount)
                .ok_or_else(|| anyhow!("output amount overflow"))
        })?;
        if output_total != input_total {
            bail!("transaction must conserve value")
        }
        Ok(())
    }

    fn validate_persisted_state(&self) -> Result<()> {
        let genesis = self
            .blocks
            .first()
            .ok_or_else(|| anyhow!("missing genesis"))?;
        if genesis.index != 0
            || genesis.previous_hash != [0; 32]
            || genesis.transactions.len() != 1
            || !genesis.validator_public_key.is_empty()
            || !genesis.validator_signature.is_empty()
        {
            bail!("invalid genesis block")
        }
        if self.validators.is_empty() {
            bail!("missing validators")
        }
        for (address, key) in &self.validators {
            let public =
                PublicKey::from_bytes(key).map_err(|_| anyhow!("invalid validator key"))?;
            if !crate::wallet::is_valid_address(address)
                || crate::wallet::address_from_public_key(public.as_bytes()) != *address
            {
                bail!("validator address does not match key")
            }
        }
        let genesis_tx = &genesis.transactions[0];
        if !genesis_tx.inputs.is_empty()
            || !genesis_tx.public_key.is_empty()
            || !genesis_tx.signature.is_empty()
        {
            bail!("invalid genesis transaction")
        }
        for output in &genesis_tx.outputs {
            validate_output(output)?;
        }
        if genesis_tx.outputs.len() > MAX_TX_OUTPUTS {
            bail!("genesis output limit exceeded")
        }
        let mut expected = BTreeMap::new();
        let genesis_id = genesis_tx.txid()?;
        for (index, output) in genesis_tx.outputs.iter().enumerate() {
            expected.insert(
                OutPoint {
                    txid: genesis_id,
                    index: index as u32,
                },
                output.clone(),
            );
        }
        for (index, block) in self.blocks.iter().enumerate().skip(1) {
            if block.transactions.len() > MAX_BLOCK_TRANSACTIONS
                || serde_json::to_vec(block)?.len() > MAX_STATE_BYTES
            {
                bail!("block size limit exceeded")
            }
            if block.index != index as u64
                || block.previous_hash != self.blocks[index - 1].hash()?
            {
                bail!("invalid chain link")
            }
            if block.timestamp < self.blocks[index - 1].timestamp {
                bail!("block timestamp rollback")
            }
            if !self
                .validators
                .values()
                .any(|key| key == &block.validator_public_key)
                || !block.verify_validator_signature()?
            {
                bail!("invalid block validator signature")
            }
            let mut spent = BTreeSet::new();
            for tx in &block.transactions {
                self.validate_transaction(tx, &expected, &spent)?;
                apply_transaction(tx, &mut expected, &mut spent)?;
            }
        }
        if expected != self.utxos {
            bail!("UTXO set does not match chain replay")
        }
        let mut pending_spent = BTreeSet::new();
        for tx in &self.pending_transactions {
            self.validate_transaction(tx, &self.utxos, &pending_spent)?;
            pending_spent.extend(tx.inputs.iter().map(|i| i.previous_output.clone()));
        }
        Ok(())
    }
}

fn validate_output(output: &TxOutput) -> Result<()> {
    if !crate::wallet::is_valid_address(&output.address) {
        bail!("invalid output address")
    }
    if output.amount == 0 {
        bail!("output amount must be nonzero")
    }
    Ok(())
}

fn apply_transaction(
    tx: &Transaction,
    utxos: &mut BTreeMap<OutPoint, TxOutput>,
    spent: &mut BTreeSet<OutPoint>,
) -> Result<()> {
    for input in &tx.inputs {
        utxos.remove(&input.previous_output);
        spent.insert(input.previous_output.clone());
    }
    let txid = tx.txid()?;
    for (index, output) in tx.outputs.iter().enumerate() {
        utxos.insert(
            OutPoint {
                txid,
                index: index as u32,
            },
            output.clone(),
        );
    }
    Ok(())
}

fn hash_bytes(bytes: &[u8]) -> Hash {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}
