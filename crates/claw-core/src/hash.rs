use crate::id::ObjectId;
use crate::object::TypeTag;

/// Domain-separated BLAKE3 hash: "claw\0" || type_tag || version || payload
pub fn content_hash(type_tag: TypeTag, payload: &[u8]) -> ObjectId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"claw\0");
    hasher.update(&[type_tag as u8]);
    hasher.update(&[1u8]); // version
    hasher.update(payload);
    let hash = hasher.finalize();
    ObjectId::from_bytes(*hash.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_hash() {
        let h1 = content_hash(TypeTag::Blob, b"hello world");
        let h2 = content_hash(TypeTag::Blob, b"hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_type_tags_produce_different_hashes() {
        let h1 = content_hash(TypeTag::Blob, b"same data");
        let h2 = content_hash(TypeTag::Tree, b"same data");
        assert_ne!(h1, h2);
    }

    #[test]
    fn different_payloads_produce_different_hashes() {
        let h1 = content_hash(TypeTag::Blob, b"data1");
        let h2 = content_hash(TypeTag::Blob, b"data2");
        assert_ne!(h1, h2);
    }
}
