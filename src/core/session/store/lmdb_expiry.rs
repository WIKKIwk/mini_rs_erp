use std::borrow::Cow;

use heed::{BoxedError, BytesDecode, BytesEncode};
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ExpiryKey {
    pub(super) expires_at_nanos: i128,
    pub(super) session_key: [u8; 32],
}

impl ExpiryKey {
    pub(super) fn new(expires_at: OffsetDateTime, session_key: [u8; 32]) -> Self {
        Self {
            expires_at_nanos: expires_at.unix_timestamp_nanos(),
            session_key,
        }
    }
}

pub(super) struct ExpiryKeyCodec;

impl<'a> BytesEncode<'a> for ExpiryKeyCodec {
    type EItem = ExpiryKey;

    fn bytes_encode(item: &'a Self::EItem) -> Result<Cow<'a, [u8]>, BoxedError> {
        let mut bytes = Vec::with_capacity(48);
        bytes.extend_from_slice(&encode_ordered_i128(item.expires_at_nanos));
        bytes.extend_from_slice(&item.session_key);
        Ok(Cow::Owned(bytes))
    }
}

impl<'a> BytesDecode<'a> for ExpiryKeyCodec {
    type DItem = ExpiryKey;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        let expires_at = bytes
            .get(..16)
            .and_then(|bytes| bytes.try_into().ok())
            .map(decode_ordered_i128)
            .ok_or("invalid expiry key timestamp")?;
        let session_key = bytes
            .get(16..48)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or("invalid expiry key session hash")?;
        Ok(ExpiryKey {
            expires_at_nanos: expires_at,
            session_key,
        })
    }
}

fn encode_ordered_i128(value: i128) -> [u8; 16] {
    ((value as u128) ^ (1_u128 << 127)).to_be_bytes()
}

fn decode_ordered_i128(bytes: [u8; 16]) -> i128 {
    (u128::from_be_bytes(bytes) ^ (1_u128 << 127)) as i128
}
