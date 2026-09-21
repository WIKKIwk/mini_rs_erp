use std::collections::BTreeMap;

use async_trait::async_trait;
use sqlx::{PgPool, Row};

use crate::core::admin::models::{AdminDirectoryEntry, AdminState};
use crate::core::admin::ports::{AdminPortError, AdminStatePort};
use crate::core::auth::access_codes::{SupplierAccessInput, supplier_access_code};
use crate::core::auth::code_vault::CodeCipher;
use crate::core::auth::password::{hash_password, is_password_hash, verify_password};
use crate::core::auth::ports::{
    AdminAccessState, AdminAccessStateLookup, AuthPortError, BuiltinLoginIdentity,
};

const CUTOVER: &str = "legacy_access_codes_v1";
const CUTOVER_LOCK: i64 = 6_514_811_918_052_126_001;

#[derive(Clone)]
pub struct PostgresAuthStore {
    pool: PgPool,
}

/// Used only by the offline cutover/bootstrap command. Never serialized or logged.
pub struct LegacyBuiltinCredential {
    pub principal_ref: String,
    pub phone: String,
    pub name: String,
    pub code: String,
}

impl PostgresAuthStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn require_ready(&self) -> Result<(), String> {
        let ready: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM mini_auth_cutovers WHERE name = $1) AND EXISTS (
                SELECT 1 FROM mini_auth_accounts a JOIN mini_auth_builtin_identities i USING (principal_ref)
                WHERE a.principal_ref = 'admin' AND a.credential_hash IS NOT NULL AND i.phone <> '')"
        ).bind(CUTOVER).fetch_one(&self.pool).await
            .map_err(|_| "credential database unavailable".to_string())?;
        if ready {
            Ok(())
        } else {
            Err("credential migration required: run mini_rs_auth_migrate before starting the server".into())
        }
    }

    /// Idempotent, all-or-nothing import. A completed cutover never reimports old codes.
    pub async fn migrate_legacy(
        &self,
        mut states: BTreeMap<String, AdminState>,
        suppliers: Vec<AdminDirectoryEntry>,
        builtins: Vec<LegacyBuiltinCredential>,
    ) -> Result<bool, String> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| "credential migration transaction failed")?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(CUTOVER_LOCK)
            .execute(&mut *tx)
            .await
            .map_err(|_| "credential migration lock failed")?;
        let completed: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM mini_auth_cutovers WHERE name = $1)")
                .bind(CUTOVER)
                .fetch_one(&mut *tx)
                .await
                .map_err(|_| "credential migration status failed")?;
        if completed {
            return Ok(false);
        }
        let occupied: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM mini_auth_accounts)")
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| "credential migration status failed")?;
        if occupied {
            return Err("credential tables are not empty; refusing to overwrite accounts".into());
        }

        for supplier in suppliers {
            let state = states.entry(supplier.ref_.clone()).or_default();
            if state.custom_code.trim().is_empty() {
                state.custom_code = supplier_access_code(&SupplierAccessInput {
                    ref_: supplier.ref_,
                    name: supplier.name,
                    phone: supplier.phone,
                })
                .map_err(|_| "legacy supplier code import failed")?;
            }
        }
        let mut identities = Vec::new();
        for builtin in builtins {
            if !matches!(
                builtin.principal_ref.as_str(),
                "admin" | "werka" | "material_taminotchi"
            ) {
                return Err("invalid builtin account".into());
            }
            let phone = crate::core::auth::service::normalize_phone(
                &builtin.phone.replace([' ', '-', '(', ')'], ""),
            )
            .map_err(|_| "invalid builtin phone")?;
            let state = states.entry(builtin.principal_ref.clone()).or_default();
            // The running legacy service used configuration for builtin credentials.
            if !builtin.code.trim().is_empty() {
                state.custom_code = builtin.code.trim().to_string();
            }
            identities.push((builtin.principal_ref, phone, builtin.name));
        }
        if !identities.iter().any(|(id, _, _)| id == "admin")
            || states
                .get("admin")
                .is_none_or(|s| s.custom_code.trim().is_empty())
        {
            return Err(
                "an admin phone and code are required for initial credential migration".into(),
            );
        }
        for (ref_, state) in states {
            let code = plaintext_code(&state);
            let (hash, data, _) = encode_state(state)
                .await
                .map_err(|_| "credential hashing failed")?;
            sqlx::query("INSERT INTO mini_auth_accounts (principal_ref, credential_hash, access_state) VALUES ($1, $2, $3)")
                .bind(&ref_).bind(&hash).bind(data).execute(&mut *tx).await.map_err(|_| "credential import failed")?;
            if let (Some(code), Some(hash)) = (code, hash) {
                save_code(&mut tx, &ref_, &hash, &code).await
                    .map_err(|_| "recoverable credential import failed")?;
            }
        }
        for (ref_, phone, name) in identities {
            sqlx::query("INSERT INTO mini_auth_builtin_identities (principal_ref, phone, display_name) VALUES ($1, $2, $3)")
                .bind(ref_).bind(phone).bind(name).execute(&mut *tx).await.map_err(|_| "builtin identity import failed")?;
        }
        sqlx::query("INSERT INTO mini_auth_cutovers (name) VALUES ($1)")
            .bind(CUTOVER)
            .execute(&mut *tx)
            .await
            .map_err(|_| "credential cutover marker failed")?;
        tx.commit()
            .await
            .map_err(|_| "credential migration commit failed")?;
        Ok(true)
    }

    pub async fn reset_admin_code(&self, code: String) -> Result<(), String> {
        self.require_ready().await?;
        let hash = hash_password(code.clone()).await?;
        let mut tx = self.pool.begin().await.map_err(|_| "admin reset transaction failed")?;
        let result = sqlx::query("UPDATE mini_auth_accounts SET credential_hash = $1, updated_at = now() WHERE principal_ref = 'admin'")
            .bind(&hash).execute(&mut *tx).await.map_err(|_| "admin credential reset failed")?;
        if result.rows_affected() != 1 {
            return Err("admin account is missing".into());
        }
        save_code(&mut tx, "admin", &hash, &code).await.map_err(|_| "admin code storage failed")?;
        tx.commit().await.map_err(|_| "admin reset commit failed")?;
        Ok(())
    }

    /// Recover only a known, still-current code; never reset a user's credential.
    pub async fn recover_access_code(&self, ref_: &str, code: &str) -> Result<bool, String> {
        let mut tx = self.pool.begin().await.map_err(|_| "credential recovery unavailable")?;
        let row = sqlx::query("SELECT a.credential_hash, EXISTS (
            SELECT 1 FROM mini_auth_code_vault v WHERE v.principal_ref = a.principal_ref
            AND v.credential_hash = a.credential_hash) AS available
            FROM mini_auth_accounts a WHERE a.principal_ref = $1 FOR UPDATE OF a")
            .bind(ref_.trim()).fetch_optional(&mut *tx).await.map_err(|_| "credential recovery lookup failed")?;
        let Some(row) = row else { return Ok(false); };
        if row.try_get::<bool, _>("available").map_err(|_| "credential recovery lookup failed")? {
            return Ok(false);
        }
        let hash = row.try_get::<Option<String>, _>("credential_hash")
            .map_err(|_| "credential recovery lookup failed")?.unwrap_or_default();
        if !verify_password(&hash, code.trim()).await? { return Ok(false); }
        save_code(&mut tx, ref_.trim(), &hash, code.trim()).await
            .map_err(|_| "credential recovery storage failed")?;
        tx.commit().await.map_err(|_| "credential recovery commit failed")?;
        Ok(true)
    }

    async fn read_identity(&self, ref_: &str) -> Result<Option<BuiltinLoginIdentity>, sqlx::Error> {
        sqlx::query(
            "SELECT phone, display_name FROM mini_auth_builtin_identities WHERE principal_ref = $1",
        )
        .bind(ref_)
        .fetch_optional(&self.pool)
        .await?
        .map(|row| {
            Ok(BuiltinLoginIdentity {
                phone: row.try_get("phone")?,
                name: row.try_get("display_name")?,
            })
        })
        .transpose()
    }
}

fn plaintext_code(state: &AdminState) -> Option<String> {
    let code = state.custom_code.trim();
    (!code.is_empty() && !is_password_hash(code)).then(|| code.to_string())
}

async fn save_code(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, ref_: &str, hash: &str, code: &str,
) -> Result<(), AdminPortError> {
    let encrypted = CodeCipher::load().and_then(|cipher| cipher.encrypt(ref_, hash, code))
        .map_err(|_| AdminPortError::LookupFailed)?;
    sqlx::query("INSERT INTO mini_auth_code_vault (principal_ref, credential_hash, encrypted_code)
        VALUES ($1, $2, $3) ON CONFLICT (principal_ref) DO UPDATE SET
        credential_hash = EXCLUDED.credential_hash, encrypted_code = EXCLUDED.encrypted_code,
        updated_at = now()")
        .bind(ref_).bind(hash).bind(encrypted).execute(&mut **tx).await
        .map_err(|_| AdminPortError::LookupFailed)?;
    Ok(())
}

async fn encode_state(
    mut state: AdminState,
) -> Result<(Option<String>, serde_json::Value, bool), AdminPortError> {
    let code = std::mem::take(&mut state.custom_code);
    let code = code.trim();
    let replace_hash = !code.is_empty() && !is_password_hash(code);
    let hash = if code.is_empty() {
        None
    } else if is_password_hash(code) {
        Some(code.to_string())
    } else {
        Some(
            hash_password(code.to_string())
                .await
                .map_err(|_| AdminPortError::LookupFailed)?,
        )
    };
    state.pending_persist_code.clear();
    state.pending_persist_at = None;
    let mut data = serde_json::to_value(state).map_err(|_| AdminPortError::LookupFailed)?;
    let object = data.as_object_mut().ok_or(AdminPortError::LookupFailed)?;
    object.remove("custom_code");
    object.remove("pending_persist_code");
    Ok((hash, data, replace_hash))
}

#[async_trait]
impl AdminStatePort for PostgresAuthStore {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        let rows = sqlx::query(
            "SELECT principal_ref, credential_hash, access_state FROM mini_auth_accounts",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;
        rows.into_iter()
            .map(|row| {
                let id: String = row
                    .try_get("principal_ref")
                    .map_err(|_| AdminPortError::LookupFailed)?;
                let data: serde_json::Value = row
                    .try_get("access_state")
                    .map_err(|_| AdminPortError::LookupFailed)?;
                let mut state: AdminState =
                    serde_json::from_value(data).map_err(|_| AdminPortError::LookupFailed)?;
                state.custom_code = row
                    .try_get::<Option<String>, _>("credential_hash")
                    .map_err(|_| AdminPortError::LookupFailed)?
                    .unwrap_or_default();
                Ok((id, state))
            })
            .collect()
    }

    async fn put_state(&self, ref_: &str, state: AdminState) -> Result<(), AdminPortError> {
        let code = plaintext_code(&state);
        let (hash, data, replace_hash) = encode_state(state).await?;
        let mut tx = self.pool.begin().await.map_err(|_| AdminPortError::LookupFailed)?;
        sqlx::query("INSERT INTO mini_auth_accounts (principal_ref, credential_hash, access_state) VALUES ($1, $2, $3)
            ON CONFLICT (principal_ref) DO UPDATE SET access_state = EXCLUDED.access_state,
            credential_hash = CASE WHEN $4 THEN EXCLUDED.credential_hash ELSE mini_auth_accounts.credential_hash END,
            updated_at = now()")
            .bind(ref_.trim()).bind(&hash).bind(data).bind(replace_hash)
            .execute(&mut *tx).await.map_err(|_| AdminPortError::LookupFailed)?;
        if let (Some(code), Some(hash)) = (code, hash) {
            save_code(&mut tx, ref_.trim(), &hash, &code).await?;
        }
        tx.commit().await.map_err(|_| AdminPortError::LookupFailed)?;
        Ok(())
    }

    async fn access_code(&self, ref_: &str) -> Result<String, AdminPortError> {
        let row = sqlx::query("SELECT v.credential_hash, v.encrypted_code
            FROM mini_auth_code_vault v JOIN mini_auth_accounts a USING (principal_ref)
            WHERE v.principal_ref = $1 AND v.credential_hash = a.credential_hash")
            .bind(ref_.trim()).fetch_optional(&self.pool).await.map_err(|_| AdminPortError::LookupFailed)?;
        let Some(row) = row else { return Ok(String::new()); };
        let hash: String = row.try_get("credential_hash").map_err(|_| AdminPortError::LookupFailed)?;
        let encrypted: String = row.try_get("encrypted_code").map_err(|_| AdminPortError::LookupFailed)?;
        CodeCipher::load().and_then(|cipher| cipher.decrypt(ref_.trim(), &hash, &encrypted))
            .map_err(|_| AdminPortError::LookupFailed)
    }

    async fn builtin_identity(
        &self,
        ref_: &str,
    ) -> Result<Option<BuiltinLoginIdentity>, AdminPortError> {
        self.read_identity(ref_)
            .await
            .map_err(|_| AdminPortError::LookupFailed)
    }

    async fn put_builtin_identity(
        &self,
        ref_: &str,
        phone: &str,
        name: &str,
    ) -> Result<(), AdminPortError> {
        let phone =
            crate::core::auth::service::normalize_phone(&phone.replace([' ', '-', '(', ')'], ""))
                .map_err(|_| AdminPortError::InvalidInput("invalid account phone".into()))?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| AdminPortError::LookupFailed)?;
        sqlx::query(
            "INSERT INTO mini_auth_accounts (principal_ref) VALUES ($1) ON CONFLICT DO NOTHING",
        )
        .bind(ref_)
        .execute(&mut *tx)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;
        sqlx::query("INSERT INTO mini_auth_builtin_identities (principal_ref, phone, display_name) VALUES ($1, $2, $3)
            ON CONFLICT (principal_ref) DO UPDATE SET phone = EXCLUDED.phone, display_name = EXCLUDED.display_name")
            .bind(ref_).bind(phone).bind(name.trim()).execute(&mut *tx).await.map_err(|_| AdminPortError::LookupFailed)?;
        tx.commit().await.map_err(|_| AdminPortError::LookupFailed)
    }
}

#[async_trait]
impl AdminAccessStateLookup for PostgresAuthStore {
    async fn remember_access_code(&self, ref_: &str, code: &str) -> Result<(), AuthPortError> {
        self.recover_access_code(ref_, code).await.map(|_| ()).map_err(|_| AuthPortError::LookupFailed)
    }

    async fn list_states(&self) -> Result<BTreeMap<String, AdminAccessState>, AuthPortError> {
        Ok(self
            .states()
            .await
            .map_err(|_| AuthPortError::LookupFailed)?
            .into_iter()
            .map(|(id, state)| {
                (
                    id,
                    AdminAccessState {
                        custom_code: state.custom_code,
                        blocked: state.blocked,
                        removed: state.removed,
                    },
                )
            })
            .collect())
    }

    async fn builtin_identity(
        &self,
        ref_: &str,
    ) -> Result<Option<BuiltinLoginIdentity>, AuthPortError> {
        self.read_identity(ref_)
            .await
            .map_err(|_| AuthPortError::LookupFailed)
    }
}
