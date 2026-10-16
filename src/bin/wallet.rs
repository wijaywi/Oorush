use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use oorush::wallet::{save_encrypted, Wallet};
use std::path::PathBuf;
use zeroize::Zeroizing;

#[derive(Parser)]
#[command(
    name = "oorush-wallet",
    about = "Create encrypted Oorush Dilithium wallets"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a new encrypted wallet. The password is read without echo.
    Generate {
        #[arg(short, long, default_value = "oorush-wallet.json")]
        output: PathBuf,
        /// Explicitly print the private key. Treat it like a password and never share it.
        #[arg(long)]
        export_private_key: bool,
    },
}

fn read_password(prompt: &str) -> Result<String> {
    rpassword::prompt_password(prompt).context("could not read password")
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Generate {
            output,
            export_private_key,
        } => {
            let password = Zeroizing::new(read_password("Wallet password: ")?);
            if password.is_empty() {
                anyhow::bail!("password must not be empty");
            }
            let confirmation = Zeroizing::new(read_password("Confirm password: ")?);
            if password != confirmation {
                anyhow::bail!("passwords do not match");
            }
            let wallet = Wallet::generate();
            save_encrypted(&wallet, &output, password.as_bytes())?;
            println!("Address: {}", wallet.address());
            println!("Public key (Dilithium5): {}", wallet.public_key_hex());
            println!("Encrypted wallet: {}", output.display());
            if export_private_key {
                eprintln!("WARNING: private-key export is sensitive. Do not log, paste, or share this value.");
                println!("Private key (Dilithium5): {}", &*wallet.secret_key_hex());
            }
        }
    }
    Ok(())
}
