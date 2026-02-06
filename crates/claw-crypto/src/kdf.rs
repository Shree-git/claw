/// Per-intent key derivation using BLAKE3's derive_key function.
/// This uses BLAKE3 in key derivation mode with a context string,
/// producing a 256-bit derived key suitable for XChaCha20-Poly1305.
pub fn derive_intent_key(master_key: &[u8; 32], intent_id: &[u8]) -> [u8; 32] {
    let context = "claw intent capsule key v1";
    let mut input = Vec::with_capacity(32 + intent_id.len());
    input.extend_from_slice(master_key);
    input.extend_from_slice(intent_id);
    blake3::derive_key(context, &input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_key_deterministic() {
        let master = [1u8; 32];
        let intent_id = b"01HQJK3B4GXVZCAQT1DWRN4A6P";

        let k1 = derive_intent_key(&master, intent_id);
        let k2 = derive_intent_key(&master, intent_id);
        assert_eq!(k1, k2);
    }

    #[test]
    fn different_intents_produce_different_keys() {
        let master = [1u8; 32];
        let k1 = derive_intent_key(&master, b"intent-a");
        let k2 = derive_intent_key(&master, b"intent-b");
        assert_ne!(k1, k2);
    }

    #[test]
    fn different_masters_produce_different_keys() {
        let m1 = [1u8; 32];
        let m2 = [2u8; 32];
        let k1 = derive_intent_key(&m1, b"intent");
        let k2 = derive_intent_key(&m2, b"intent");
        assert_ne!(k1, k2);
    }
}
