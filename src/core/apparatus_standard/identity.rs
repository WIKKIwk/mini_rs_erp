use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

const APPARATUS_ID_PREFIX: &str = "apparatus:";
const MAX_ID_LENGTH: usize = 128;

/// Stable, opaque apparatus identity.
///
/// Display names and aliases never participate in validation or resolution.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApparatusId(String);

impl ApparatusId {
    pub fn new(value: impl Into<String>) -> Result<Self, ApparatusIdError> {
        let value = value.into();
        validate_shape(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for ApparatusId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ApparatusId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for ApparatusId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ApparatusId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ApparatusIdError {
    #[error("apparatus id is empty")]
    Empty,
    #[error("apparatus id must use the canonical apparatus:<namespace>:<opaque-key> shape")]
    InvalidShape,
    #[error("apparatus id contains whitespace or control characters")]
    InvalidCharacters,
}

fn validate_shape(value: &str) -> Result<(), ApparatusIdError> {
    if value.trim().is_empty() {
        return Err(ApparatusIdError::Empty);
    }
    if value != value.trim()
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(ApparatusIdError::InvalidCharacters);
    }
    if value.len() > MAX_ID_LENGTH || !value.starts_with(APPARATUS_ID_PREFIX) {
        return Err(ApparatusIdError::InvalidShape);
    }
    let segments = value[APPARATUS_ID_PREFIX.len()..]
        .split(':')
        .collect::<Vec<_>>();
    if segments.len() != 2 || segments.iter().any(|segment| segment.is_empty()) {
        return Err(ApparatusIdError::InvalidShape);
    }
    if segments.iter().any(|segment| {
        segment.chars().any(|character| {
            !(character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_' | '.'))
        })
    }) {
        return Err(ApparatusIdError::InvalidShape);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_identity_is_shape_validated_but_not_compared_with_display_text() {
        assert_eq!(
            ApparatusId::new("apparatus:catalog:stable-001")
                .unwrap()
                .as_str(),
            "apparatus:catalog:stable-001"
        );
        assert!(ApparatusId::new("apparatus:catalog:flexo-pechat").is_ok());
        assert_eq!(
            ApparatusId::new("apparatus:legacy"),
            Err(ApparatusIdError::InvalidShape)
        );
        assert_eq!(ApparatusId::new(" "), Err(ApparatusIdError::Empty));
    }
}
