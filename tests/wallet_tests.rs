use oorush::wallet::{address_from_public_key, load_encrypted, save_encrypted, Wallet};
use sha2::{Digest, Sha256};
use std::fs;

#[test]
fn address_derivation_is_deterministic_and_domain_separated() {
    let public_key = b"test public key";
    let first = address_from_public_key(public_key);
    assert_eq!(first, address_from_public_key(public_key));
    let mut hash = Sha256::new();
    hash.update(b"OORUSH::ADDRESS::DILITHIUM5::v1");
    hash.update(public_key);
    assert_eq!(first, format!("oorush1{}", hex::encode(hash.finalize())));
}

#[test]
fn encrypted_wallet_round_trip_and_wrong_password_failure() {
    let path =
        std::env::temp_dir().join(format!("oorush-wallet-{}-{}.json", std::process::id(), 1));
    let _ = fs::remove_file(&path);
    let wallet = Wallet::generate();
    save_encrypted(&wallet, &path, b"correct horse battery staple").unwrap();
    let loaded = load_encrypted(&path, b"correct horse battery staple").unwrap();
    assert_eq!(loaded.public_key, wallet.public_key);
    assert_eq!(
        loaded.secret_key_hex().as_str(),
        wallet.secret_key_hex().as_str()
    );
    assert!(load_encrypted(&path, b"wrong password").is_err());
    let _ = fs::remove_file(path);
}

#[test]
fn wallet_save_never_overwrites_existing_file() {
    let path =
        std::env::temp_dir().join(format!("oorush-wallet-{}-{}.json", std::process::id(), 2));
    let _ = fs::remove_file(&path);
    let wallet = Wallet::generate();
    save_encrypted(&wallet, &path, b"password").unwrap();
    let original = fs::read(&path).unwrap();
    assert!(save_encrypted(&Wallet::generate(), &path, b"password").is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    let _ = fs::remove_file(path);
}

#[test]
fn wallet_debug_redacts_secret_key() {
    let wallet = Wallet::generate();
    let debug = format!("{wallet:?}");
    let secret = wallet.secret_key_hex();
    assert!(debug.contains("[REDACTED]"));
    assert!(!debug.contains(&*secret));
}
