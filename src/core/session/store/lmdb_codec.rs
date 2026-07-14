use std::borrow::Cow;

use heed::{BoxedError, BytesDecode, BytesEncode};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::session::models::SessionRecord;

pub(super) struct SessionRecordCodec;

const SESSION_RECORD_MAGIC: &[u8] = b"AMS2";

impl<'a> BytesEncode<'a> for SessionRecordCodec {
    type EItem = SessionRecord;

    fn bytes_encode(item: &'a Self::EItem) -> Result<Cow<'a, [u8]>, BoxedError> {
        let payload = bincode::serialize(&StoredSessionRecord::from_record(item))?;
        let mut bytes = Vec::with_capacity(SESSION_RECORD_MAGIC.len() + payload.len());
        bytes.extend_from_slice(SESSION_RECORD_MAGIC);
        bytes.extend_from_slice(&payload);
        Ok(Cow::Owned(bytes))
    }
}

impl<'a> BytesDecode<'a> for SessionRecordCodec {
    type DItem = SessionRecord;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        if let Some(payload) = bytes.strip_prefix(SESSION_RECORD_MAGIC) {
            let stored: StoredSessionRecord = bincode::deserialize(payload)?;
            return stored.into_record();
        }
        Ok(serde_json::from_slice(bytes)?)
    }
}

#[derive(Serialize, Deserialize)]
struct StoredSessionRecord {
    principal: StoredPrincipal,
    created_at_nanos: Option<i128>,
    updated_at_nanos: Option<i128>,
    expires_at_nanos: Option<i128>,
}

impl StoredSessionRecord {
    fn from_record(record: &SessionRecord) -> Self {
        Self {
            principal: StoredPrincipal::from_principal(&record.principal),
            created_at_nanos: record.created_at.map(OffsetDateTime::unix_timestamp_nanos),
            updated_at_nanos: record.updated_at.map(OffsetDateTime::unix_timestamp_nanos),
            expires_at_nanos: record.expires_at.map(OffsetDateTime::unix_timestamp_nanos),
        }
    }

    fn into_record(self) -> Result<SessionRecord, BoxedError> {
        Ok(SessionRecord {
            principal: self.principal.into_principal()?,
            created_at: decode_timestamp(self.created_at_nanos)?,
            updated_at: decode_timestamp(self.updated_at_nanos)?,
            expires_at: decode_timestamp(self.expires_at_nanos)?,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct StoredPrincipal {
    role: u8,
    display_name: String,
    legal_name: String,
    ref_: String,
    phone: String,
    avatar_url: String,
}

impl StoredPrincipal {
    fn from_principal(principal: &Principal) -> Self {
        Self {
            role: encode_role(&principal.role),
            display_name: principal.display_name.clone(),
            legal_name: principal.legal_name.clone(),
            ref_: principal.ref_.clone(),
            phone: principal.phone.clone(),
            avatar_url: principal.avatar_url.clone(),
        }
    }

    fn into_principal(self) -> Result<Principal, BoxedError> {
        Ok(Principal {
            role: decode_role(self.role)?,
            display_name: self.display_name,
            legal_name: self.legal_name,
            ref_: self.ref_,
            phone: self.phone,
            avatar_url: self.avatar_url,
        })
    }
}

fn encode_role(role: &PrincipalRole) -> u8 {
    match role {
        PrincipalRole::Supplier => 0,
        PrincipalRole::Werka => 1,
        PrincipalRole::Customer => 2,
        PrincipalRole::Admin => 3,
        PrincipalRole::Aparatchi => 4,
        PrincipalRole::Qolipchi => 5,
        PrincipalRole::MaterialTaminotchi => 6,
        PrincipalRole::Boyoqchi => 7,
    }
}

fn decode_role(role: u8) -> Result<PrincipalRole, BoxedError> {
    match role {
        0 => Ok(PrincipalRole::Supplier),
        1 => Ok(PrincipalRole::Werka),
        2 => Ok(PrincipalRole::Customer),
        3 => Ok(PrincipalRole::Admin),
        4 => Ok(PrincipalRole::Aparatchi),
        5 => Ok(PrincipalRole::Qolipchi),
        6 => Ok(PrincipalRole::MaterialTaminotchi),
        7 => Ok(PrincipalRole::Boyoqchi),
        _ => Err("invalid stored session principal role".into()),
    }
}

fn decode_timestamp(timestamp: Option<i128>) -> Result<Option<OffsetDateTime>, BoxedError> {
    timestamp
        .map(OffsetDateTime::from_unix_timestamp_nanos)
        .transpose()
        .map_err(Into::into)
}
