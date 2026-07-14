use serde::{Deserialize, Serialize};

use crate::core::werka::models::WerkaHomeData;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalRole {
    Supplier,
    Werka,
    Customer,
    Aparatchi,
    Qolipchi,
    Boyoqchi,
    MaterialTaminotchi,
    Admin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub role: PrincipalRole,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub legal_name: String,
    #[serde(rename = "ref")]
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ref_: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub phone: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub avatar_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginRequest {
    pub phone: String,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub profile: Principal,
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assigned_apparatus: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assigned_item_groups: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assigned_warehouses: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub werka_home: Option<WerkaHomeData>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::core::werka::models::{WerkaHomeData, WerkaHomeSummary};

    fn werka_profile() -> Principal {
        Principal {
            role: PrincipalRole::Werka,
            display_name: "Werka".to_string(),
            legal_name: "Werka".to_string(),
            ref_: "werka".to_string(),
            phone: "+99888862440".to_string(),
            avatar_url: String::new(),
        }
    }

    #[test]
    fn login_response_omits_missing_werka_home_like_go() {
        let response = LoginResponse {
            token: "token".to_string(),
            profile: werka_profile(),
            capabilities: Vec::new(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: Vec::new(),
            assigned_warehouses: Vec::new(),
            werka_home: None,
        };

        let value = serde_json::to_value(response).expect("serialize login response");

        assert!(value.get("werka_home").is_none());
    }

    #[test]
    fn login_response_serializes_werka_home_go_shape() {
        let response = LoginResponse {
            token: "token".to_string(),
            profile: werka_profile(),
            capabilities: vec!["werka.access".to_string()],
            assigned_apparatus: Vec::new(),
            assigned_item_groups: Vec::new(),
            assigned_warehouses: Vec::new(),
            werka_home: Some(WerkaHomeData {
                summary: WerkaHomeSummary {
                    pending_count: 2,
                    confirmed_count: 3,
                    returned_count: 1,
                },
                pending_items: Vec::new(),
            }),
        };

        let value = serde_json::to_value(response).expect("serialize login response");

        assert_eq!(
            value["werka_home"],
            json!({
                "summary": {
                    "pending_count": 2,
                    "confirmed_count": 3,
                    "returned_count": 1
                },
                "pending_items": []
            })
        );
    }

    #[test]
    fn login_response_serializes_assigned_item_groups() {
        let response = LoginResponse {
            token: "token".to_string(),
            profile: werka_profile(),
            capabilities: Vec::new(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: vec!["Kraska".to_string(), "Kley".to_string()],
            assigned_warehouses: Vec::new(),
            werka_home: None,
        };

        let value = serde_json::to_value(response).expect("serialize login response");

        assert_eq!(value["assigned_item_groups"], json!(["Kraska", "Kley"]));
    }

    #[test]
    fn login_response_serializes_assigned_warehouses() {
        let response = LoginResponse {
            token: "token".to_string(),
            profile: werka_profile(),
            capabilities: Vec::new(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: Vec::new(),
            assigned_warehouses: vec!["Xomashyo".to_string()],
            werka_home: None,
        };

        let value = serde_json::to_value(response).expect("serialize login response");

        assert_eq!(value["assigned_warehouses"], json!(["Xomashyo"]));
    }
}
