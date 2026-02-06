use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;

use crate::CryptoError;

pub struct KeyPair {
    signing_key: SigningKey,
}

impl KeyPair {
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        Self { signing_key }
    }

    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self, CryptoError> {
        let signing_key = SigningKey::from_bytes(bytes);
        Ok(Self { signing_key })
    }

    pub fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.signing_key.to_bytes()
    }

    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.verifying_key().to_bytes()
    }

    pub fn save_to_file(&self, path: &std::path::Path) -> Result<(), CryptoError> {
        std::fs::write(path, self.to_bytes())?;
        Ok(())
    }

    pub fn load_from_file(path: &std::path::Path) -> Result<Self, CryptoError> {
        let bytes = std::fs::read(path)?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| CryptoError::InvalidKey("expected 32 bytes".into()))?;
        Self::from_bytes(&arr)
    }
}
