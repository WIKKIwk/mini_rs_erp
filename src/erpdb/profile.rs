use async_trait::async_trait;
use sqlx::query_as;

use crate::core::profile::ports::{
    CustomerProfileRecord, DownloadedFile, ProfileLookup, ProfilePortError, SupplierProfileRecord,
};
use crate::erpdb::reader::DirectDbReader;

#[async_trait]
impl ProfileLookup for DirectDbReader {
    async fn get_supplier_profile(
        &self,
        id: &str,
    ) -> Result<SupplierProfileRecord, ProfilePortError> {
        let row = query_as::<_, SupplierProfileRow>(SUPPLIER_PROFILE_SQL)
            .bind(id.trim())
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| ProfilePortError::LookupFailed)?
            .ok_or(ProfilePortError::LookupFailed)?;
        Ok(SupplierProfileRecord {
            phone: profile_phone(&row.mobile_no, &row.supplier_details),
            image: row.image.trim().to_string(),
        })
    }

    async fn get_customer_profile(
        &self,
        id: &str,
    ) -> Result<CustomerProfileRecord, ProfilePortError> {
        let row = query_as::<_, CustomerProfileRow>(CUSTOMER_PROFILE_SQL)
            .bind(id.trim())
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| ProfilePortError::LookupFailed)?
            .ok_or(ProfilePortError::LookupFailed)?;
        Ok(CustomerProfileRecord {
            phone: profile_phone(&row.mobile_no, &row.customer_details),
        })
    }

    async fn download_file(&self, _file_url: &str) -> Result<DownloadedFile, ProfilePortError> {
        Err(ProfilePortError::LookupFailed)
    }

    async fn upload_supplier_image(
        &self,
        _supplier_id: &str,
        _filename: &str,
        _content_type: &str,
        _content: Vec<u8>,
    ) -> Result<String, ProfilePortError> {
        Err(ProfilePortError::LookupFailed)
    }
}

#[derive(sqlx::FromRow)]
struct SupplierProfileRow {
    mobile_no: String,
    supplier_details: String,
    image: String,
}

#[derive(sqlx::FromRow)]
struct CustomerProfileRow {
    mobile_no: String,
    customer_details: String,
}

fn profile_phone(mobile_no: &str, details: &str) -> String {
    if !mobile_no.trim().is_empty() {
        return mobile_no.trim().to_string();
    }
    extract_phone_from_details(details)
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

const SUPPLIER_PROFILE_SQL: &str = r#"
    SELECT
        COALESCE(s.mobile_no, '') AS mobile_no,
        COALESCE(s.supplier_details, '') AS supplier_details,
        COALESCE(s.image, '') AS image
    FROM tabSupplier s
    WHERE s.name = ?
    LIMIT 1
"#;

const CUSTOMER_PROFILE_SQL: &str = r#"
    SELECT
        COALESCE(c.mobile_no, '') AS mobile_no,
        COALESCE(c.customer_details, '') AS customer_details
    FROM tabCustomer c
    WHERE c.name = ?
    LIMIT 1
"#;

#[cfg(test)]
mod tests {
    use super::{extract_phone_from_details, profile_phone};

    #[test]
    fn profile_phone_prefers_mobile_no_like_erpnext_lookup() {
        assert_eq!(profile_phone(" +99890 ", "Phone: +99891"), "+99890");
    }

    #[test]
    fn profile_phone_falls_back_to_details_like_erpnext_lookup() {
        assert_eq!(
            extract_phone_from_details("old\r\nPhone: +998901234567"),
            "+998901234567"
        );
        assert_eq!(
            extract_phone_from_details("Telefon: +998901234568"),
            "+998901234568"
        );
    }
}
