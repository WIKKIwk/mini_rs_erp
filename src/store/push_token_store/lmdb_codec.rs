use std::borrow::Cow;

use heed::{BoxedError, BytesDecode, BytesEncode};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::core::push::models::PushTokenRecord;

pub(super) struct PushTokenRecordsCodec;

const PUSH_TOKEN_RECORDS_MAGIC: &[u8] = b"AMT1";

impl<'a> BytesEncode<'a> for PushTokenRecordsCodec {
    type EItem = Vec<PushTokenRecord>;

    fn bytes_encode(item: &'a Self::EItem) -> Result<Cow<'a, [u8]>, BoxedError> {
        let records: Vec<StoredPushTokenRecord> = item
            .iter()
            .map(StoredPushTokenRecord::from_record)
            .collect();
        let payload = bincode::serialize(&records)?;
        let mut bytes = Vec::with_capacity(PUSH_TOKEN_RECORDS_MAGIC.len() + payload.len());
        bytes.extend_from_slice(PUSH_TOKEN_RECORDS_MAGIC);
        bytes.extend_from_slice(&payload);
        Ok(Cow::Owned(bytes))
    }
}

impl<'a> BytesDecode<'a> for PushTokenRecordsCodec {
    type DItem = Vec<PushTokenRecord>;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        if let Some(payload) = bytes.strip_prefix(PUSH_TOKEN_RECORDS_MAGIC) {
            let records: Vec<StoredPushTokenRecord> = bincode::deserialize(payload)?;
            return records
                .into_iter()
                .map(StoredPushTokenRecord::into_record)
                .collect();
        }
        Ok(serde_json::from_slice(bytes)?)
    }
}

#[derive(Serialize, Deserialize)]
struct StoredPushTokenRecord {
    token: String,
    platform: String,
    updated_at_nanos: i128,
}

impl StoredPushTokenRecord {
    fn from_record(record: &PushTokenRecord) -> Self {
        Self {
            token: record.token.clone(),
            platform: record.platform.clone(),
            updated_at_nanos: record.updated_at.unix_timestamp_nanos(),
        }
    }

    fn into_record(self) -> Result<PushTokenRecord, BoxedError> {
        Ok(PushTokenRecord {
            token: self.token,
            platform: self.platform,
            updated_at: OffsetDateTime::from_unix_timestamp_nanos(self.updated_at_nanos)?,
        })
    }
}
