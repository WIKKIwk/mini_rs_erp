use async_trait::async_trait;
use serde::Deserialize;

use crate::core::auth::ports::{AuthPortError, CustomerLookup, CustomerRecord};
use crate::core::profile::ports::{CustomerProfileRecord, ProfilePortError};
use crate::erpnext::client::ErpnextClient;

#[async_trait]
impl CustomerLookup for ErpnextClient {
    async fn search_customers(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<CustomerRecord>, AuthPortError> {
        let limit = normalize_limit(limit);
        let mut request = self
            .http
            .get(format!("{}/api/resource/Customer", self.base_url()))
            .header(reqwest::header::AUTHORIZATION, self.auth_header().await)
            .query(&[
                (
                    "fields",
                    r#"["name","customer_name","mobile_no","customer_details"]"#,
                ),
                ("filters", r#"[["disabled","=",0]]"#),
                ("limit_page_length", &limit.to_string()),
                ("order_by", "modified desc"),
            ]);

        let trimmed = query.trim();
        if !trimmed.is_empty() {
            let like = format!("%{}%", trimmed.replace('"', ""));
            let or_filters = serde_json::json!([
                ["name", "like", like],
                ["customer_name", "like", like],
                ["mobile_no", "like", like],
            ]);
            request = request.query(&[("or_filters", or_filters.to_string())]);
        }

        let payload = request
            .send()
            .await
            .map_err(|_| AuthPortError::LookupFailed)?
            .error_for_status()
            .map_err(|_| AuthPortError::LookupFailed)?
            .json::<CustomerListResponse>()
            .await
            .map_err(|_| AuthPortError::LookupFailed)?;

        Ok(customers_from_list_response(payload))
    }
}

pub async fn get_customer_profile(
    client: &ErpnextClient,
    id: &str,
) -> Result<CustomerProfileRecord, ProfilePortError> {
    let payload = client
        .http
        .get(format!(
            "{}/api/resource/Customer/{}",
            client.base_url(),
            urlencoding::encode(id.trim())
        ))
        .header(reqwest::header::AUTHORIZATION, client.auth_header().await)
        .send()
        .await
        .map_err(|_| ProfilePortError::LookupFailed)?
        .error_for_status()
        .map_err(|_| ProfilePortError::LookupFailed)?
        .json::<CustomerGetResponse>()
        .await
        .map_err(|_| ProfilePortError::LookupFailed)?;

    Ok(CustomerProfileRecord {
        phone: if opt_str(&payload.data.mobile_no).trim().is_empty() {
            extract_phone_from_details(opt_str(&payload.data.customer_details))
        } else {
            opt_str(&payload.data.mobile_no).trim().to_string()
        },
    })
}

fn normalize_limit(limit: usize) -> usize {
    match limit {
        0 => 20,
        1..=500 => limit,
        _ => 500,
    }
}

#[derive(Debug, Deserialize)]
struct CustomerListResponse {
    data: Vec<CustomerListRow>,
}

#[derive(Debug, Deserialize)]
struct CustomerListRow {
    name: String,
    #[serde(default)]
    customer_name: Option<String>,
    #[serde(default)]
    mobile_no: Option<String>,
    #[serde(default)]
    customer_details: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CustomerGetResponse {
    data: CustomerGetRow,
}

#[derive(Debug, Deserialize)]
struct CustomerGetRow {
    #[serde(default)]
    mobile_no: Option<String>,
    #[serde(default)]
    customer_details: Option<String>,
}

fn customers_from_list_response(payload: CustomerListResponse) -> Vec<CustomerRecord> {
    payload
        .data
        .into_iter()
        .map(|row| {
            let customer_name = opt_str(&row.customer_name);
            let mobile_no = opt_str(&row.mobile_no);
            let customer_details = opt_str(&row.customer_details);
            let name = if customer_name.trim().is_empty() {
                row.name.trim().to_string()
            } else {
                customer_name.trim().to_string()
            };
            let phone = if mobile_no.trim().is_empty() {
                extract_phone_from_details(customer_details)
            } else {
                mobile_no.trim().to_string()
            };

            CustomerRecord {
                id: row.name.trim().to_string(),
                name,
                phone,
            }
        })
        .collect()
}

fn opt_str(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("")
}

fn extract_phone_from_details(details: &str) -> String {
    for line in details.replace("\r\n", "\n").lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        if lower.starts_with("telefon:") {
            return trimmed["telefon:".len()..].trim().to_string();
        }
        if lower.starts_with("phone:") {
            return trimmed["phone:".len()..].trim().to_string();
        }
    }

    String::new()
}

#[cfg(test)]
mod tests {
    use super::{CustomerListResponse, CustomerListRow, customers_from_list_response};

    #[test]
    fn maps_customer_name_and_phone_details_like_go() {
        let customers = customers_from_list_response(CustomerListResponse {
            data: vec![CustomerListRow {
                name: "CUST-001".to_string(),
                customer_name: None,
                mobile_no: None,
                customer_details: Some("Phone: +998901234567".to_string()),
            }],
        });

        assert_eq!(customers[0].id, "CUST-001");
        assert_eq!(customers[0].name, "CUST-001");
        assert_eq!(customers[0].phone, "+998901234567");
    }

    #[test]
    fn accepts_null_optional_customer_fields_like_erpnext() {
        let payload: CustomerListResponse = serde_json::from_value(serde_json::json!({
            "data": [{
                "name": "CUST-001",
                "customer_name": null,
                "mobile_no": "+998901234567",
                "customer_details": null
            }]
        }))
        .expect("customer response");

        let customers = customers_from_list_response(payload);
        assert_eq!(customers[0].name, "CUST-001");
        assert_eq!(customers[0].phone, "+998901234567");
    }
}
