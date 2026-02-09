use claw_core::id::ObjectId;
use claw_core::types::{Capsule, CapsulePublic, CapsuleSignature};

use crate::encrypt;
use crate::keypair::KeyPair;
use crate::sign;
use crate::CryptoError;

pub fn build_capsule(
    revision_id: &ObjectId,
    public_fields: CapsulePublic,
    private_data: Option<&[u8]>,
    encryption_key: Option<&[u8; 32]>,
    signing_keypair: &KeyPair,
) -> Result<Capsule, CryptoError> {
    // Encrypt private data if provided
    let encrypted_private = match (private_data, encryption_key) {
        (Some(data), Some(key)) => Some(encrypt::encrypt(key, data)?),
        _ => None,
    };

    // Build signing payload: revision_id || public_hash || encrypted_hash
    let public_bytes = serde_json::to_vec(&public_fields)
        .map_err(|e| CryptoError::SigningFailed(e.to_string()))?;
    let public_hash = blake3::hash(&public_bytes);

    let mut sign_payload = Vec::new();
    sign_payload.extend_from_slice(revision_id.as_bytes());
    sign_payload.extend_from_slice(public_hash.as_bytes());
    if let Some(enc) = &encrypted_private {
        let enc_hash = blake3::hash(enc);
        sign_payload.extend_from_slice(enc_hash.as_bytes());
    }

    let sig = sign::sign(signing_keypair, &sign_payload);

    let encryption = if encryption_key.is_some() {
        "xchacha20poly1305".to_string()
    } else {
        String::new()
    };

    Ok(Capsule {
        revision_id: *revision_id,
        public_fields,
        encrypted_private,
        encryption,
        key_id: None,
        signatures: vec![CapsuleSignature {
            signer_id: hex::encode(sig.signer_id),
            signature: sig.signature,
        }],
    })
}

pub fn verify_capsule(capsule: &Capsule, public_key: &[u8; 32]) -> Result<bool, CryptoError> {
    let sig = capsule
        .signatures
        .first()
        .ok_or_else(|| CryptoError::VerificationFailed("no signature".into()))?;

    let public_bytes = serde_json::to_vec(&capsule.public_fields)
        .map_err(|e| CryptoError::VerificationFailed(e.to_string()))?;
    let public_hash = blake3::hash(&public_bytes);

    let mut sign_payload = Vec::new();
    sign_payload.extend_from_slice(capsule.revision_id.as_bytes());
    sign_payload.extend_from_slice(public_hash.as_bytes());
    if let Some(enc) = &capsule.encrypted_private {
        let enc_hash = blake3::hash(enc);
        sign_payload.extend_from_slice(enc_hash.as_bytes());
    }

    crate::verify::verify(public_key, &sign_payload, &sig.signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use claw_core::hash::content_hash;
    use claw_core::object::TypeTag;

    fn test_public() -> CapsulePublic {
        CapsulePublic {
            agent_id: "test-agent".to_string(),
            agent_version: None,
            toolchain_digest: None,
            env_fingerprint: None,
            evidence: vec![],
        }
    }

    #[test]
    fn capsule_sign_and_verify() {
        let kp = KeyPair::generate();
        let rev_id = content_hash(TypeTag::Revision, b"test revision");

        let capsule = build_capsule(&rev_id, test_public(), None, None, &kp).unwrap();
        let pk = kp.public_key_bytes();
        assert!(verify_capsule(&capsule, &pk).unwrap());
    }

    #[test]
    fn capsule_tamper_detection() {
        let kp = KeyPair::generate();
        let rev_id = content_hash(TypeTag::Revision, b"test revision");

        let mut capsule = build_capsule(&rev_id, test_public(), None, None, &kp).unwrap();
        // Tamper with the public fields
        capsule.public_fields.agent_id = "TAMPERED".to_string();
        let pk = kp.public_key_bytes();
        assert!(!verify_capsule(&capsule, &pk).unwrap());
    }

    #[test]
    fn capsule_with_encrypted_private() {
        let kp = KeyPair::generate();
        let rev_id = content_hash(TypeTag::Revision, b"test");
        let enc_key = [99u8; 32];

        let private_data = b"secret private data";
        let capsule = build_capsule(
            &rev_id,
            test_public(),
            Some(private_data),
            Some(&enc_key),
            &kp,
        )
        .unwrap();

        assert!(capsule.encrypted_private.is_some());
        assert_eq!(capsule.encryption, "xchacha20poly1305");
        let pk = kp.public_key_bytes();
        assert!(verify_capsule(&capsule, &pk).unwrap());

        // Can decrypt
        let decrypted =
            encrypt::decrypt(&enc_key, capsule.encrypted_private.as_ref().unwrap()).unwrap();
        assert_eq!(decrypted, private_data);

        // Wrong key can't decrypt
        let wrong_key = [100u8; 32];
        assert!(encrypt::decrypt(&wrong_key, capsule.encrypted_private.as_ref().unwrap()).is_err());
    }
}
