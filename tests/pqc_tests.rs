// tests/pqc_tests.rs
// Simple test for Dilithium PoC

#[cfg(feature = "pqc-dilithium")]
mod tests {
    use oorush::pqc::dilithium_precompile::{generate_keypair, sign_detached, verify_dilithium};

    #[test]
    fn test_dilithium_signature_roundtrip() {
        let (pk, sk) = generate_keypair();
        let msg = b"hello oorush PQC!";
        let sig = sign_detached(&sk, msg).expect("should sign");
        let ok = verify_dilithium(&pk, msg, &sig).expect("should verify");
        assert!(ok, "valid signature must verify");
    }

    #[test]
    fn test_dilithium_signature_tampered() {
        let (pk, sk) = generate_keypair();
        let msg = b"this message will be tampered";
        let mut sig = sign_detached(&sk, msg).expect("should sign");
        // corrupt the signature
        sig[0] ^= 0xFF;
        let ok = verify_dilithium(&pk, msg, &sig).expect("should verify");
        assert!(!ok, "tampered signature must fail");
    }
}

#[cfg(feature = "pqc-kyber")]
mod kyber_tests {
    use oorush::pqc::kyber_kem::{decapsulate_secret, encapsulate_secret, generate_keypair};
    use pqcrypto_traits::kem::SharedSecret;

    #[test]
    fn test_kyber_encapsulate_decapsulate() {
        let (pk, sk) = generate_keypair();

        // Alice encapsulates a secret for Bob using Bob's public key
        let (ct, alice_shared_secret) = encapsulate_secret(&pk);

        // Bob decapsulates the ciphertext using his secret key
        let bob_shared_secret = decapsulate_secret(&ct, &sk);

        // The shared secrets must match
        assert_eq!(
            alice_shared_secret.as_bytes(),
            bob_shared_secret.as_bytes(),
            "Shared secrets should match"
        );
    }
}

#[cfg(test)]
mod piano_tests {
    use oorush::piano::generate_from_seed;
    use std::fs;

    #[test]
    fn piano_metadata_changes_with_seed() {
        let a_path = std::env::temp_dir().join(format!("oorush-test-a-{}.wav", std::process::id()));
        let b_path = std::env::temp_dir().join(format!("oorush-test-b-{}.wav", std::process::id()));
        let a = generate_from_seed(b"seed-a", &a_path).unwrap();
        let b = generate_from_seed(b"seed-b", &b_path).unwrap();
        assert_ne!(a.seed_hash, b.seed_hash);
        assert_ne!(a.audio_sha256, b.audio_sha256);
        assert_eq!(&fs::read(&a_path).unwrap()[0..4], b"RIFF");
        let _ = fs::remove_file(a_path);
        let _ = fs::remove_file(b_path);
    }
}
