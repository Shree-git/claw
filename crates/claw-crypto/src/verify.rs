use ed25519_dalek::{Signature, Verifier, VerifyingKey};

use crate::CryptoError;

pub fn verify(
    public_key_bytes: &[u8; 32],
    data: &[u8],
    signature: &[u8],
) -> Result<bool, CryptoError> {
    let verifying_key = VerifyingKey::from_bytes(public_key_bytes)
        .map_err(|e| CryptoError::InvalidKey(e.to_string()))?;

    let sig_bytes: [u8; 64] = signature
        .try_into()
        .map_err(|_| CryptoError::VerificationFailed("invalid signature length".into()))?;

    let sig = Signature::from_bytes(&sig_bytes);

    match verifying_key.verify(data, &sig) {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}
