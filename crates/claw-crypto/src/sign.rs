use ed25519_dalek::Signer;

use crate::keypair::KeyPair;

pub struct Signature {
    pub signer_id: Vec<u8>,
    pub signature: Vec<u8>,
}

pub fn sign(keypair: &KeyPair, data: &[u8]) -> Signature {
    let sig = keypair.signing_key().sign(data);
    Signature {
        signer_id: keypair.public_key_bytes().to_vec(),
        signature: sig.to_bytes().to_vec(),
    }
}
