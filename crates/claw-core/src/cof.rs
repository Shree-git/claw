use crate::error::CoreError;
use crate::object::TypeTag;

const MAGIC: &[u8; 4] = b"CLW1";
const COF_VERSION: u8 = 0x01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Compression {
    None = 0x00,
    Zstd = 0x01,
}

impl Compression {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x00 => Some(Self::None),
            0x01 => Some(Self::Zstd),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CofFlags(u8);

impl CofFlags {
    pub fn new(compressed: bool, encrypted: bool) -> Self {
        let mut bits = 0u8;
        if compressed {
            bits |= 0x01;
        }
        if encrypted {
            bits |= 0x02;
        }
        Self(bits)
    }

    pub fn bits(&self) -> u8 {
        self.0
    }

    pub fn is_compressed(&self) -> bool {
        self.0 & 0x01 != 0
    }

    pub fn is_encrypted(&self) -> bool {
        self.0 & 0x02 != 0
    }
}

fn encode_uvarint(mut value: u64, buf: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn decode_uvarint(data: &[u8], pos: &mut usize) -> Result<u64, CoreError> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    loop {
        if *pos >= data.len() {
            return Err(CoreError::Deserialization(
                "unexpected end of uvarint".into(),
            ));
        }
        let byte = data[*pos];
        *pos += 1;
        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            return Err(CoreError::Deserialization("uvarint overflow".into()));
        }
    }
    Ok(result)
}

/// Encode a payload into COF v1 format.
/// Format: [4B magic][1B version][1B type_tag][1B flags][1B compression][uvarint uncompressed_len][payload][4B CRC32]
pub fn cof_encode(type_tag: TypeTag, payload: &[u8]) -> Result<Vec<u8>, CoreError> {
    let compression = if payload.len() > 64 {
        Compression::Zstd
    } else {
        Compression::None
    };

    let compressed = match compression {
        Compression::None => payload.to_vec(),
        Compression::Zstd => {
            zstd::encode_all(payload, 3).map_err(|e| CoreError::Compression(e.to_string()))?
        }
    };

    let flags = CofFlags::new(
        compression != Compression::None,
        false, // encryption flag set at higher layer
    );

    let mut buf = Vec::with_capacity(4 + 4 + compressed.len() + 10 + 4);

    // Header
    buf.extend_from_slice(MAGIC);
    buf.push(COF_VERSION);
    buf.push(type_tag as u8);
    buf.push(flags.bits());
    buf.push(compression as u8);

    // Uncompressed length
    encode_uvarint(payload.len() as u64, &mut buf);

    // Payload
    buf.extend_from_slice(&compressed);

    // CRC32 of uncompressed payload (little endian) per spec
    let crc = crc32fast::hash(payload);
    buf.extend_from_slice(&crc.to_le_bytes());

    Ok(buf)
}

/// Decode COF v1 format, returning (TypeTag, decompressed payload).
pub fn cof_decode(data: &[u8]) -> Result<(TypeTag, Vec<u8>), CoreError> {
    if data.len() < 12 {
        return Err(CoreError::Deserialization("data too short for COF".into()));
    }

    // Check magic
    if &data[..4] != MAGIC {
        return Err(CoreError::InvalidMagic);
    }

    // Version
    let version = data[4];
    if version != COF_VERSION {
        return Err(CoreError::UnsupportedVersion(version));
    }

    // Type tag
    let type_tag = TypeTag::from_u8(data[5]).ok_or(CoreError::UnknownTypeTag(data[5]))?;

    // Flags (data[6]) - reserved for future use
    let _flags = data[6];

    // Compression
    let compression = Compression::from_u8(data[7])
        .ok_or_else(|| CoreError::Deserialization(format!("unknown compression: {}", data[7])))?;

    // Uncompressed length
    let mut pos = 8;
    let uncompressed_len = decode_uvarint(data, &mut pos)? as usize;

    // CRC32 check: last 4 bytes
    if data.len() < pos + 4 {
        return Err(CoreError::Deserialization(
            "data too short for CRC32".into(),
        ));
    }
    let crc_offset = data.len() - 4;
    let expected_crc = u32::from_le_bytes([
        data[crc_offset],
        data[crc_offset + 1],
        data[crc_offset + 2],
        data[crc_offset + 3],
    ]);

    // Compressed payload
    let compressed = &data[pos..crc_offset];

    // Decompress
    let payload = match compression {
        Compression::None => compressed.to_vec(),
        Compression::Zstd => {
            let mut decompressed = zstd::decode_all(compressed)
                .map_err(|e| CoreError::Decompression(e.to_string()))?;
            if decompressed.len() != uncompressed_len {
                decompressed.truncate(uncompressed_len);
            }
            decompressed
        }
    };

    // CRC32 of uncompressed payload per spec
    let actual_crc = crc32fast::hash(&payload);
    if expected_crc != actual_crc {
        return Err(CoreError::Crc32Mismatch {
            expected: expected_crc,
            actual: actual_crc,
        });
    }

    Ok((type_tag, payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_no_compression() {
        let payload = b"short";
        let encoded = cof_encode(TypeTag::Blob, payload).unwrap();
        let (tag, decoded) = cof_decode(&encoded).unwrap();
        assert_eq!(tag, TypeTag::Blob);
        assert_eq!(decoded, payload);
    }

    #[test]
    fn roundtrip_with_compression() {
        let payload = vec![b'a'; 1000];
        let encoded = cof_encode(TypeTag::Tree, &payload).unwrap();
        let (tag, decoded) = cof_decode(&encoded).unwrap();
        assert_eq!(tag, TypeTag::Tree);
        assert_eq!(decoded, payload);
    }

    #[test]
    fn crc_corruption_detected() {
        let payload = b"test data";
        let mut encoded = cof_encode(TypeTag::Blob, payload).unwrap();
        // Corrupt one byte in the payload area
        let mid = encoded.len() / 2;
        encoded[mid] ^= 0xFF;
        assert!(cof_decode(&encoded).is_err());
    }

    #[test]
    fn invalid_magic_rejected() {
        let mut data = cof_encode(TypeTag::Blob, b"test").unwrap();
        data[0] = b'X';
        assert!(matches!(cof_decode(&data), Err(CoreError::InvalidMagic)));
    }

    #[test]
    fn all_type_tags_roundtrip() {
        for tag_val in 0x01..=0x0Cu8 {
            let tag = TypeTag::from_u8(tag_val).unwrap();
            let payload = format!("payload for {}", tag.name());
            let encoded = cof_encode(tag, payload.as_bytes()).unwrap();
            let (decoded_tag, decoded_payload) = cof_decode(&encoded).unwrap();
            assert_eq!(decoded_tag, tag);
            assert_eq!(decoded_payload, payload.as_bytes());
        }
    }
}
