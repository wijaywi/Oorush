use anyhow::Result;
use oorush::piano;
use oorush::pqc::dilithium_precompile;
use pqcrypto_traits::sign::{PublicKey, SecretKey};
use std::path::PathBuf;

fn main() -> Result<()> {
    println!("=== Oorush Quantum-Resistant Cryptography Library ===");
    println!("This binary demonstrates Dilithium signatures and piano output.\n");
    let (pk, sk) = dilithium_precompile::generate_keypair();
    println!("Public Key generated ({} bytes)", pk.as_bytes().len());
    println!("Secret Key generated ({} bytes)", sk.as_bytes().len());
    let message = b"Hello, Quantum World!";
    let signature = dilithium_precompile::sign_detached(&sk, message)?;
    let valid = dilithium_precompile::verify_dilithium(&pk, message, &signature)?;
    println!("Signature verification: {}", valid);

    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let output = args
        .windows(2)
        .find(|pair| pair[0] == "--output")
        .map(|pair| PathBuf::from(&pair[1]))
        .unwrap_or_else(|| PathBuf::from("oorush_piano.wav"));
    let seed_text = args
        .windows(2)
        .find(|pair| pair[0] == "--seed")
        .map(|pair| pair[1].to_string_lossy().into_owned());
    let seed = seed_text
        .as_deref()
        .map(str::as_bytes)
        .unwrap_or_else(|| pk.as_bytes());
    let music = piano::generate_from_seed(seed, &output)?;
    println!("Piano WAV: {}", output.display());
    println!(
        "Melody: {} notes, {} BPM, {} ms",
        music.note_count, music.tempo_bpm, music.duration_ms
    );
    println!("Audio SHA-256: {}", music.audio_sha256);
    Ok(())
}
