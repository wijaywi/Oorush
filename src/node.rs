//! Small, deliberately educational TCP node for the blockchain MVP.
//!
//! The wire format is one UTF-8 JSON object per line. Requests are
//! `{"command":"get_status"}`, `get_balance` (with `address`),
//! `submit_transaction` (with `transaction`), `produce_block`, and
//! `get_chain`. A peer may also use `submit_block` to propagate a block.
//! Every response is one line: `{"ok":true,"result":...}` or
//! `{"ok":false,"error":"..."}`. Lines over 1 MiB are rejected.
//! This is a PoA MVP: it has no fork choice, Byzantine fault tolerance, or
//! production-grade authentication. The transport is intentionally unauthenticated;
//! keep the listener on loopback until transport authentication is added.

use crate::{
    blockchain::{Block, ChainState, Transaction},
    wallet,
};
use anyhow::{anyhow, Context, Result};
use rpassword::prompt_password;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_LINE: usize = 1024 * 1024;
const MAX_RESPONSE: usize = 8 * 1024 * 1024;
const MAX_WORKERS: usize = 32;
const MAX_QUEUED_CONNECTIONS: usize = 128;
const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum Request {
    GetStatus,
    GetBalance { address: String },
    SubmitTransaction { transaction: Transaction },
    ProduceBlock,
    GetChain,
    SubmitBlock { block: Block },
}

#[derive(Clone)]
pub struct NodeConfig {
    pub listen: String,
    pub state: PathBuf,
    pub wallet: PathBuf,
    pub initial_balance: u64,
    pub peer: Option<String>,
}

struct Node {
    state: Mutex<ChainState>,
    wallet: wallet::Wallet,
    state_path: PathBuf,
    peer: Option<String>,
}

pub fn run(config: NodeConfig) -> Result<()> {
    let password =
        prompt_password("Wallet password: ").context("could not read wallet password")?;
    if password.is_empty() {
        anyhow::bail!("wallet password must not be empty");
    }
    let signing_wallet = wallet::load_encrypted(&config.wallet, password.as_bytes())?;
    let chain = if config.state.exists() || backup_path(&config.state).exists() {
        load_state(&config.state)?
    } else {
        let chain = ChainState::create_genesis(
            vec![crate::blockchain::TxOutput {
                address: signing_wallet.address(),
                amount: config.initial_balance,
            }],
            vec![signing_wallet.public_key.clone()],
            0,
        )?;
        // create_new ensures a pre-existing state is never silently replaced.
        let bytes = chain.serialize()?;
        use std::fs::OpenOptions;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&config.state)
            .with_context(|| {
                format!(
                    "create state {} (existing files are never overwritten)",
                    config.state.display()
                )
            })?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        chain
    };
    let address: SocketAddr = config
        .listen
        .parse()
        .context("node listen address must be an IP socket address")?;
    if !address.ip().is_loopback() {
        anyhow::bail!("unauthenticated node must listen on localhost; use a loopback address")
    }
    if let Some(peer) = &config.peer {
        let peer_address: SocketAddr = peer
            .parse()
            .context("peer address must be an IP socket address")?;
        if !peer_address.ip().is_loopback() {
            anyhow::bail!("unauthenticated peer connections must remain on localhost")
        }
    }
    let listener = TcpListener::bind(address).context("bind node listener")?;
    let node = Arc::new(Node {
        state: Mutex::new(chain),
        wallet: signing_wallet,
        state_path: config.state,
        peer: config.peer,
    });
    let (connections, receiver) = mpsc::sync_channel::<TcpStream>(MAX_QUEUED_CONNECTIONS);
    let receiver = Arc::new(Mutex::new(receiver));
    for _ in 0..MAX_WORKERS {
        let receiver = Arc::clone(&receiver);
        let node = Arc::clone(&node);
        thread::spawn(move || {
            while let Ok(stream) = receiver.lock().unwrap().recv() {
                let _ = handle_client(stream, Arc::clone(&node));
            }
        });
    }
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                if connections.try_send(stream).is_err() {
                    // The queue is bounded; a busy node does not create an
                    // unbounded thread or retain unbounded socket state.
                    eprintln!("node connection queue full; rejecting connection");
                }
            }
            Err(error) => eprintln!("node connection error: {error}"),
        }
    }
    Ok(())
}

fn handle_client(mut stream: TcpStream, node: Arc<Node>) -> Result<()> {
    let mut line = String::new();
    let mut reader = BufReader::new(stream.try_clone()?.take((MAX_LINE + 1) as u64));
    if reader.read_line(&mut line)? > MAX_LINE {
        return write_error(&mut stream, "request too large");
    }
    if !line.ends_with('\n') && line.len() >= MAX_LINE {
        return write_error(&mut stream, "request too large");
    }
    let request: Request = match serde_json::from_str(line.trim_end()) {
        Ok(r) => r,
        Err(e) => return write_error(&mut stream, &format!("invalid request: {e}")),
    };
    let result = dispatch(request, &node);
    match result {
        Ok(value) => write_response(&mut stream, json!({"ok": true, "result": value})),
        Err(e) => write_error(&mut stream, &e.to_string()),
    }
}

fn dispatch(request: Request, node: &Node) -> Result<serde_json::Value> {
    match request {
        Request::GetStatus => {
            let state = node.state.lock().unwrap();
            Ok(
                json!({"height": state.blocks.len() - 1, "pending": state.pending_transactions.len(), "validators": state.validators.len()}),
            )
        }
        Request::GetBalance { address } => {
            let state = node.state.lock().unwrap();
            Ok(json!({"address": address, "balance": state.balance(&address)?}))
        }
        Request::GetChain => {
            let state = node.state.lock().unwrap();
            Ok(serde_json::to_value(&state.blocks)?)
        }
        Request::SubmitTransaction { transaction } => {
            let mut state = node.state.lock().unwrap();
            let id = mutate_and_persist(&mut state, &node.state_path, |next| {
                next.submit_transaction(transaction)
            })?;
            Ok(json!({"txid": id}))
        }
        Request::SubmitBlock { block } => {
            let mut state = node.state.lock().unwrap();
            mutate_and_persist(&mut state, &node.state_path, |next| {
                next.validate_and_append_block(block)
            })?;
            Ok(json!({"height": state.blocks.len() - 1}))
        }
        Request::ProduceBlock => {
            let block = {
                let mut state = node.state.lock().unwrap();
                mutate_and_persist(&mut state, &node.state_path, |next| {
                    let txs = next.pending_transactions.clone();
                    let block = next.produce_block(txs, node.wallet.secret_key_bytes(), now())?;
                    next.validate_and_append_block(block.clone())?;
                    Ok(block)
                })?
            };
            if let Some(peer) = &node.peer {
                broadcast(peer, &block);
            }
            Ok(json!(block))
        }
    }
}

fn persist(state: &ChainState, path: &Path) -> Result<()> {
    let bytes = state.serialize()?;
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    let backup = path.with_extension("bak");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .with_context(|| format!("create temporary state {}", temp.display()))?;
    let result = (|| -> Result<()> {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        if path.exists() {
            let _ = fs::remove_file(&backup);
            fs::rename(path, &backup).context("backup chain state")?;
        }
        if let Err(error) = fs::rename(&temp, path) {
            if backup.exists() {
                let _ = fs::rename(&backup, path);
            }
            return Err(error).context("install chain state");
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn mutate_and_persist<T, F>(state: &mut ChainState, path: &Path, mutation: F) -> Result<T>
where
    F: FnOnce(&mut ChainState) -> Result<T>,
{
    let mut next = state.clone();
    let result = mutation(&mut next)?;
    persist(&next, path)?;
    *state = next;
    Ok(result)
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("bak")
}

fn load_state(path: &Path) -> Result<ChainState> {
    let primary_error = match fs::read(path) {
        Ok(primary) => match validate_loaded_state(&primary) {
            Ok(state) => return Ok(state),
            Err(error) => error,
        },
        Err(error) => anyhow!("read primary chain state: {error}"),
    };
    let backup = backup_path(path);
    let bytes = fs::read(&backup).with_context(|| {
        format!(
            "invalid primary chain state ({primary_error}); read recovery backup {}",
            backup.display()
        )
    })?;
    validate_loaded_state(&bytes).context("validate recovery chain state")
}

fn validate_loaded_state(bytes: &[u8]) -> Result<ChainState> {
    let state = ChainState::deserialize(bytes).context("decode chain state")?;
    let genesis = state
        .blocks
        .first()
        .ok_or_else(|| anyhow::anyhow!("state has no genesis block"))?;
    if genesis.index != 0
        || !genesis.previous_hash.iter().all(|b| *b == 0)
        || !genesis.validator_public_key.is_empty()
        || !genesis.validator_signature.is_empty()
    {
        anyhow::bail!("invalid genesis block")
    }
    let allocations = genesis
        .transactions
        .first()
        .ok_or_else(|| anyhow::anyhow!("genesis has no transaction"))?
        .outputs
        .clone();
    let mut rebuilt = ChainState::create_genesis(
        allocations,
        state.validators.values().cloned().collect(),
        genesis.timestamp,
    )?;
    for block in state.blocks.iter().skip(1).cloned() {
        rebuilt.validate_and_append_block(block)?;
    }
    for tx in state.pending_transactions.iter().cloned() {
        rebuilt.submit_transaction(tx)?;
    }
    if rebuilt.blocks != state.blocks
        || rebuilt.utxos != state.utxos
        || rebuilt.validators != state.validators
        || rebuilt.pending_transactions != state.pending_transactions
    {
        anyhow::bail!("chain state does not match validated chain")
    }
    Ok(state)
}
fn broadcast(peer: &str, block: &Block) {
    let Ok(address) = peer.parse::<SocketAddr>() else {
        eprintln!("invalid peer address; expected an IP socket address");
        return;
    };
    if let Ok(mut stream) = TcpStream::connect_timeout(&address, IO_TIMEOUT) {
        let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
        let _ = writeln!(
            stream,
            "{}",
            json!({"command":"submit_block", "block": block})
        );
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn write_response(stream: &mut TcpStream, value: serde_json::Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(&value).context("serialize response")?;
    if bytes.len() > MAX_RESPONSE {
        return write_error(stream, "response too large");
    }
    bytes.push(b'\n');
    stream.write_all(&bytes).context("write response")
}
fn write_error(stream: &mut TcpStream, error: &str) -> Result<()> {
    write_response(stream, json!({"ok": false, "error": error}))
}

#[derive(Debug, Serialize)]
pub struct _ProtocolMarker {
    pub max_request_bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("oorush-node-{label}-{}", std::process::id()))
    }

    #[test]
    fn malformed_state_is_rejected_without_a_valid_backup() {
        let path = test_path("malformed.json");
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(backup_path(&path));
        fs::write(&path, b"not chain state").unwrap();
        assert!(load_state(&path).is_err());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn missing_primary_state_recovers_from_valid_backup() {
        let path = test_path("missing-primary.json");
        let backup = backup_path(&path);
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&backup);
        let wallet = wallet::Wallet::generate();
        let state = ChainState::create_genesis(
            vec![crate::blockchain::TxOutput {
                address: wallet.address(),
                amount: 1,
            }],
            vec![wallet.public_key],
            0,
        )
        .unwrap();
        fs::write(&backup, state.serialize().unwrap()).unwrap();
        let recovered = load_state(&path).unwrap();
        assert_eq!(recovered.blocks, state.blocks);
        let _ = fs::remove_file(path);
        let _ = fs::remove_file(backup);
    }

    #[test]
    fn failed_persistence_does_not_commit_mutated_state() {
        let wallet = wallet::Wallet::generate();
        let mut state = ChainState::create_genesis(
            vec![crate::blockchain::TxOutput {
                address: wallet.address(),
                amount: 1,
            }],
            vec![wallet.public_key],
            0,
        )
        .unwrap();
        let before = state.serialize().unwrap();
        let path = test_path("missing-parent").join("chain.json");
        let result = mutate_and_persist(&mut state, &path, |next| {
            next.pending_transactions.clear();
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(state.serialize().unwrap(), before);
    }
}
