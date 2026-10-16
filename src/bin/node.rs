use clap::Parser;
use oorush::node::{run, NodeConfig};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "oorush-node",
    about = "Educational Oorush proof-of-authority node"
)]
struct Cli {
    #[arg(long, default_value = "127.0.0.1:18444")]
    listen: String,
    /// Optional read-only HTTP status API. It is restricted to loopback.
    #[arg(long)]
    http_listen: Option<String>,
    #[arg(long, default_value = "oorush-chain.json")]
    state: PathBuf,
    #[arg(long, default_value = "oorush-wallet.json")]
    wallet: PathBuf,
    #[arg(long, default_value_t = 1_000_000)]
    initial_balance: u64,
    #[arg(long)]
    peer: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    run(NodeConfig {
        listen: cli.listen,
        http_listen: cli.http_listen,
        state: cli.state,
        wallet: cli.wallet,
        initial_balance: cli.initial_balance,
        peer: cli.peer,
    })
}
