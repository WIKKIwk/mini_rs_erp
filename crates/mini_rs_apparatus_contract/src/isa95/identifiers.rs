use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use super::CanonicalApparatusValidationError;

const MAX_SEMANTIC_ID_BYTES: usize = 256;

macro_rules! semantic_id {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(
                value: impl Into<String>,
            ) -> Result<Self, CanonicalApparatusValidationError> {
                let value = value.into();
                validate_semantic_id(&value, $label)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(de::Error::custom)
            }
        }
    };
}

semantic_id!(EquipmentClassId, "equipment_class_id");
semantic_id!(PhysicalAssetId, "physical_asset_id");
semantic_id!(HierarchyLevelId, "hierarchy_level_id");

fn validate_semantic_id(
    value: &str,
    field: &'static str,
) -> Result<(), CanonicalApparatusValidationError> {
    if value.is_empty()
        || value != value.trim()
        || value.len() > MAX_SEMANTIC_ID_BYTES
        || value.chars().any(char::is_control)
        || value.chars().any(char::is_whitespace)
        || !value.contains(':')
    {
        return Err(CanonicalApparatusValidationError::InvalidIdentifier(field));
    }
    Ok(())
}
