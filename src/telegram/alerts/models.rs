use super::super::models::TelegramUserGroup;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    RawMaterial,
    Qolip,
}

impl AlertKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::RawMaterial => "raw_material",
            Self::Qolip => "qolip",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::RawMaterial => "Material ta’minotchi",
            Self::Qolip => "Qolipchi",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "raw_material" => Some(Self::RawMaterial),
            "qolip" => Some(Self::Qolip),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlertMember {
    pub user_id: i64,
    pub display_name: String,
    pub username: String,
    #[serde(default)]
    pub access_hash: Option<i64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TelegramAlertSettings {
    #[serde(default)]
    pub sender_user_id: Option<String>,
    #[serde(default)]
    pub group: Option<TelegramUserGroup>,
    #[serde(default)]
    pub raw_material_members: Vec<AlertMember>,
    #[serde(default)]
    pub qolip_members: Vec<AlertMember>,
}

impl TelegramAlertSettings {
    pub fn members(&self, kind: AlertKind) -> &[AlertMember] {
        match kind {
            AlertKind::RawMaterial => &self.raw_material_members,
            AlertKind::Qolip => &self.qolip_members,
        }
    }
    pub fn members_mut(&mut self, kind: AlertKind) -> &mut Vec<AlertMember> {
        match kind {
            AlertKind::RawMaterial => &mut self.raw_material_members,
            AlertKind::Qolip => &mut self.qolip_members,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct AlertJob {
    pub id: String,
    pub sender_user_id: String,
    pub group: TelegramUserGroup,
    pub message: String,
    pub members: Vec<AlertMember>,
    pub random_id: i64,
    pub attempts: u32,
    pub next_attempt_at: i64,
    pub delivered: bool,
    pub last_error: Option<String>,
}

#[derive(Deserialize)]
pub struct AlertSenderUpdate {
    #[serde(default)]
    pub sender_user_id: Option<String>,
}

#[derive(Clone)]
pub(crate) struct MemberPage {
    pub members: Vec<AlertMember>,
    pub has_more: bool,
}
