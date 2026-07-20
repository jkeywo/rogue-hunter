//! Share-code encoding: postcard bytes + CRC32, base64url, `RH1-` prefix.
//!
//! The format lives in `vellum-digest`; the prefix, and therefore the identity
//! of a rogue-hunter share code, lives here.

use vellum_digest::{CodecError, ShareCodec};

use crate::{ReplayError, ReplayRecord};

/// `RH1-` is rogue-hunter, share-code format 1. It is what stops a code from
/// the other game decoding into something that looks plausible.
const CODEC: ShareCodec = ShareCodec::new("RH1-");

pub fn encode(record: &ReplayRecord) -> String {
    // Encoding a record we just built cannot fail; an empty string is a
    // visibly broken code rather than a panic in a release build.
    CODEC.encode(record).unwrap_or_default()
}

pub fn decode(code: &str) -> Result<ReplayRecord, ReplayError> {
    CODEC.decode(code).map_err(|error| match error {
        CodecError::WrongPrefix { .. } => ReplayError::Malformed("missing RH1- prefix".to_owned()),
        CodecError::NotBase64(detail) => ReplayError::Malformed(format!("base64: {detail}")),
        CodecError::TooShort => ReplayError::Malformed("payload too short".to_owned()),
        CodecError::ChecksumMismatch => ReplayError::Malformed("checksum mismatch".to_owned()),
        CodecError::Payload(detail) => ReplayError::Malformed(format!("payload: {detail}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The published CRC-32 check value, asserted here as well as in vellum:
    /// every existing share code carries this checksum, so an engine change
    /// that moved it must fail in the game that depends on it.
    #[test]
    fn crc32_matches_known_vector() {
        assert_eq!(vellum_digest::crc32_ieee(b"123456789"), 0xCBF43926);
    }

    /// The prefix is part of the format. Changing it invalidates every code a
    /// player has saved, so it is pinned rather than merely used.
    #[test]
    fn the_prefix_is_rh1() {
        assert_eq!(CODEC.prefix(), "RH1-");
    }
}
