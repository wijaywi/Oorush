//! Oorush-native Dilithium wallet storage and address derivation.
//!
//! Addresses are deliberately not Bitcoin/Ethereum compatible.  An address is
//! `oorush1` followed by lowercase hex SHA-256 of
//! `OORUSH::ADDRESS::DILITHIUM5::v1` and the canonical public-key bytes.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{anyhow, Context, Result};
use argon2::{Argon2, Params, Version};
use pqcrypto_dilithium::dilithium5::{PublicKey, SecretKey};
use pqcrypto_traits::sign::{PublicKey as PublicKeyTrait, SecretKey as SecretKeyTrait};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};
use zeroize::Zeroizing;

const ADDRESS_DOMAIN: &[u8] = b"OORUSH::ADDRESS::DILITHIUM5::v1";
const KDF_M_COST: u32 = 19_456;
const KDF_T_COST: u32 = 2;
const KDF_P_COST: u32 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_CHECK_MESSAGE: &[u8] = b"OORUSH::WALLET::KEY_CHECK::v1";

#[derive(Clone)]
pub struct Wallet {
    pub public_key: Vec<u8>,
    pub secret_key: Zeroizing<Vec<u8>>,
}

impl std::fmt::Debug for Wallet {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Wallet")
            .field("public_key", &hex::encode(&self.public_key))
            .field("secret_key", &"[REDACTED]")
            .finish()
    }
}

impl Wallet {
    pub fn generate() -> Self {
        let (public_key, secret_key) = crate::pqc::dilithium_precompile::generate_keypair();
        Self {
            public_key: public_key.as_bytes().to_vec(),
            secret_key: Zeroizing::new(secret_key.as_bytes().to_vec()),
        }
    }

    pub fn address(&self) -> String {
        address_from_public_key(&self.public_key)
    }

    pub fn public_key_hex(&self) -> String {
        hex::encode(&self.public_key)
    }

    pub fn secret_key_hex(&self) -> Zeroizing<String> {
        Zeroizing::new(hex::encode(self.secret_key.as_slice()))
    }
}

pub fn address_from_public_key(public_key: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ADDRESS_DOMAIN);
    hasher.update(public_key);
    format!("oorush1{}", hex::encode(hasher.finalize()))
}

pub fn is_valid_address(address: &str) -> bool {
    address.len() == 71
        && address.starts_with("oorush1")
        && address[7..]
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[derive(Serialize, Deserialize)]
struct FileFormat {
    format: u8,
    scheme: String,
    address: String,
    public_key: String,
    kdf: KdfFormat,
    cipher: CipherFormat,
}
#[derive(Serialize, Deserialize)]
struct KdfFormat {
    name: String,
    salt: String,
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
}
#[derive(Serialize, Deserialize)]
struct CipherFormat {
    name: String,
    nonce: String,
    ciphertext: String,
}

fn derive_key(password: &[u8], salt: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    let params = Params::new(KDF_M_COST, KDF_T_COST, KDF_P_COST, Some(32))
        .map_err(|e| anyhow!("invalid KDF parameters: {e}"))?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(argon2::Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password, salt, key.as_mut())
        .map_err(|e| anyhow!("password derivation failed: {e}"))?;
    Ok(key)
}

// Kept separate to make the encryption format easy to audit.
pub fn save_encrypted(wallet: &Wallet, path: &Path, password: &[u8]) -> Result<()> {
    if password.is_empty() {
        return Err(anyhow!("password must not be empty"));
    }
    validate_keypair(wallet)?;
    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let key = derive_key(password, &salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_ref()));
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce_bytes),
            wallet.secret_key.as_slice(),
        )
        .map_err(|_| anyhow!("wallet encryption failed"))?;
    let file = FileFormat {
        format: 1,
        scheme: "dilithium5".into(),
        address: wallet.address(),
        public_key: wallet.public_key_hex(),
        kdf: KdfFormat {
            name: "argon2id".into(),
            salt: hex::encode(salt),
            memory_kib: KDF_M_COST,
            iterations: KDF_T_COST,
            parallelism: KDF_P_COST,
        },
        cipher: CipherFormat {
            name: "aes-256-gcm".into(),
            nonce: hex::encode(nonce_bytes),
            ciphertext: hex::encode(ciphertext),
        },
    };
    let bytes = serde_json::to_vec_pretty(&file)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| {
            format!(
                "cannot create wallet {} (existing files are never overwritten)",
                path.display()
            )
        })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        output.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    if let Err(error) = output.write_all(&bytes).and_then(|_| output.sync_all()) {
        drop(output);
        let _ = fs::remove_file(path);
        return Err(error.into());
    }
    Ok(())
}

pub fn load_encrypted(path: &Path, password: &[u8]) -> Result<Wallet> {
    const MAX_WALLET_FILE_BYTES: u64 = 64 * 1024;
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_WALLET_FILE_BYTES {
        return Err(anyhow!("wallet file is too large"));
    }
    let bytes = fs::read(path)?;
    if bytes.len() as u64 > MAX_WALLET_FILE_BYTES {
        return Err(anyhow!("wallet file is too large"));
    }
    let file: FileFormat = serde_json::from_slice(&bytes)?;
    if file.format != 1
        || file.scheme != "dilithium5"
        || file.kdf.name != "argon2id"
        || file.kdf.memory_kib != KDF_M_COST
        || file.kdf.iterations != KDF_T_COST
        || file.kdf.parallelism != KDF_P_COST
        || file.cipher.name != "aes-256-gcm"
    {
        return Err(anyhow!("unsupported wallet format"));
    }
    let public_key = hex::decode(file.public_key)?;
    let salt = hex::decode(file.kdf.salt)?;
    let nonce = hex::decode(file.cipher.nonce)?;
    let ciphertext = hex::decode(file.cipher.ciphertext)?;
    if salt.len() != SALT_LEN || nonce.len() != NONCE_LEN {
        return Err(anyhow!("invalid wallet parameters"));
    }
    let key = derive_key(password, &salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_ref()));
    let secret = Zeroizing::new(
        cipher
            .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
            .map_err(|_| anyhow!("wrong password or corrupted wallet"))?,
    );
    let secret_key =
        SecretKey::from_bytes(&secret).map_err(|_| anyhow!("invalid secret key in wallet"))?;
    let public =
        PublicKey::from_bytes(&public_key).map_err(|_| anyhow!("invalid public key in wallet"))?;
    let candidate = Wallet {
        public_key: public.as_bytes().to_vec(),
        secret_key: Zeroizing::new(secret_key.as_bytes().to_vec()),
    };
    validate_keypair(&candidate)?;
    if address_from_public_key(public.as_bytes()) != file.address {
        return Err(anyhow!("wallet address does not match public key"));
    }
    Ok(candidate)
}

fn validate_keypair(wallet: &Wallet) -> Result<()> {
    let public = PublicKey::from_bytes(&wallet.public_key)
        .map_err(|_| anyhow!("invalid Dilithium public key"))?;
    let secret = SecretKey::from_bytes(wallet.secret_key.as_slice())
        .map_err(|_| anyhow!("invalid Dilithium secret key"))?;
    let signature = crate::pqc::dilithium_precompile::sign_detached(&secret, KEY_CHECK_MESSAGE)?;
    if !crate::pqc::dilithium_precompile::verify_dilithium(&public, KEY_CHECK_MESSAGE, &signature)?
    {
        return Err(anyhow!("public key does not match secret key"));
    }
    Ok(())
}
