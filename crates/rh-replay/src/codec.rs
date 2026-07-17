//! Share-code encoding: postcard bytes + CRC32, base64url, `RH1-` prefix.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;

use crate::{ReplayError, ReplayRecord};

const PREFIX: &str = "RH1-";

pub fn encode(record: &ReplayRecord) -> String {
    let mut bytes = postcard::to_allocvec(record).unwrap_or_default();
    let crc = crc32(&bytes);
    bytes.extend_from_slice(&crc.to_le_bytes());
    format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes))
}

pub fn decode(code: &str) -> Result<ReplayRecord, ReplayError> {
    let trimmed = code.trim();
    let payload = trimmed
        .strip_prefix(PREFIX)
        .ok_or_else(|| ReplayError::Malformed("missing RH1- prefix".to_owned()))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|error| ReplayError::Malformed(format!("base64: {error}")))?;
    if bytes.len() < 4 {
        return Err(ReplayError::Malformed("payload too short".to_owned()));
    }
    let (body, crc_bytes) = bytes.split_at(bytes.len() - 4);
    let expected = u32::from_le_bytes([crc_bytes[0], crc_bytes[1], crc_bytes[2], crc_bytes[3]]);
    if crc32(body) != expected {
        return Err(ReplayError::Malformed("checksum mismatch".to_owned()));
    }
    postcard::from_bytes(body).map_err(|error| ReplayError::Malformed(format!("payload: {error}")))
}

/// CRC-32 (IEEE 802.3), bitwise implementation; guards against typos in
/// hand-copied share codes.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_known_vector() {
        // CRC-32 of "123456789" is the classic check value 0xCBF43926.
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }
}
