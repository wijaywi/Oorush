//! Small, deliberately educational TCP node for the blockchain MVP.
//!
//! The wire format is one UTF-8 JSON object per line. Requests are
//! `{"command":"get_status"}`, `get_balance` (with `address`),
//! `submit_transaction` (with `transaction`), `produce_block`, and
//! `get_chain`. A peer may also use `submit_block` to propagate a block.
//! Every response is one line: `{"ok":true,"result":...}` or
//! `{"ok":false,"error":"..."}`. Lines over 1 MiB are rejected.
//! This is a PoA MVP: it has no fork choice, Byzantine fault tolerance, or
//! production-grade authentication.

use crate::{
    blockchain::{Block, ChainState, Transaction},
    wallet,
};
use anyhow::{Context, Result};
use rpassword::prompt_password;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_LINE: usize = 1024 * 1024;

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
    let chain = if config.state.exists() {
        ChainState::deserialize(&fs::read(&config.state).context("read chain state")?)?
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
    let listener = TcpListener::bind(&config.listen).context("bind node listener")?;
    let node = Arc::new(Node {
        state: Mutex::new(chain),
        wallet: signing_wallet,
        state_path: config.state,
        peer: config.peer,
    });
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let node = Arc::clone(&node);
                thread::spawn(move || {
                    let _ = handle_client(stream, node);
                });
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
            let id = state.submit_transaction(transaction)?;
            persist(&state, &node.state_path)?;
            Ok(json!({"txid": id}))
        }
        Request::SubmitBlock { block } => {
            let mut state = node.state.lock().unwrap();
            state.validate_and_append_block(block)?;
            persist(&state, &node.state_path)?;
            Ok(json!({"height": state.blocks.len() - 1}))
        }
        Request::ProduceBlock => {
            let block = {
                let mut state = node.state.lock().unwrap();
                let txs = state.pending_transactions.clone();
                let block = state.produce_block(txs, node.wallet.secret_key.as_slice(), now())?;
                state.validate_and_append_block(block.clone())?;
                persist(&state, &node.state_path)?;
                block
            };
            if let Some(peer) = &node.peer {
                broadcast(peer, &block);
            }
            Ok(json!(block))
        }
    }
}

fn persist(state: &ChainState, path: &Path) -> Result<()> {
    fs::write(path, state.serialize()?).context("save chain state")
}
fn broadcast(peer: &str, block: &Block) {
    if let Ok(mut stream) = TcpStream::connect(peer) {
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
    writeln!(stream, "{value}").context("write response")
}
fn write_error(stream: &mut TcpStream, error: &str) -> Result<()> {
    write_response(stream, json!({"ok": false, "error": error}))
}

#[derive(Debug, Serialize)]
pub struct _ProtocolMarker {
    pub max_request_bytes: usize,
}
