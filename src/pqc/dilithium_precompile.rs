use anyhow::{anyhow, Result};
use pqcrypto_dilithium::dilithium5::{
    detached_sign, keypair, verify_detached_signature,
    DetachedSignature as ConcreteDetachedSignature, PublicKey, SecretKey,
};
use pqcrypto_traits::sign::DetachedSignature;

/// Generates a new Dilithium keypair.
pub fn generate_keypair() -> (PublicKey, SecretKey) {
    keypair()
}

/// Signs a message using a detached signature.
pub fn sign_detached(sk: &SecretKey, msg: &[u8]) -> Result<Vec<u8>> {
    let sig = detached_sign(msg, sk);
    Ok(sig.as_bytes().to_vec())
}

/// Verifies a detached Dilithium signature.
pub fn verify_dilithium(pk: &PublicKey, msg: &[u8], sig: &[u8]) -> Result<bool> {
    let signature = ConcreteDetachedSignature::from_bytes(sig)
        .map_err(|_| anyhow!("Invalid signature bytes"))?;

    // verify_detached_signature returns Result<(), VerificationError>
    // We map it to a boolean to match the test cases
    let is_valid = verify_detached_signature(&signature, msg, pk).is_ok();
    Ok(is_valid)
}
