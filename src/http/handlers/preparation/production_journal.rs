use super::*;

#[derive(Debug, Default, Deserialize)]
pub struct ProductionJournalQuery {
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub order_id: String,
    #[serde(default)]
    pub batch_id: String,
    #[serde(default)]
    pub offset: usize,
    pub limit: Option<usize>,
}

impl ProductionJournalQuery {
    fn validate(&self) -> Result<(), ApiError> {
        if self.q.chars().count() > 200
            || self.order_id.len() > 512
            || self.batch_id.len() > 1024
            || self.offset > 1_000_000
            || self.limit.is_some_and(|limit| limit == 0 || limit > 100)
            || !matches!(self.status.as_str(), "" | "all" | "active" | "completed")
        {
            return Err(error(PreparationError::Invalid("Jurnal filtri noto‘g‘ri")));
        }
        Ok(())
    }
}

pub async fn production_orders(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ProductionJournalQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    query.validate()?;
    store(&state)?
        .production_journal_orders(
            &actor.ref_,
            &query.q,
            if query.status.is_empty() {
                "all"
            } else {
                &query.status
            },
            query.offset,
            query.limit.unwrap_or(50),
        )
        .await
        .map(Json)
        .map_err(error)
}

pub async fn production_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ProductionJournalQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    query.validate()?;
    if query.order_id.trim().is_empty() {
        return Err(error(PreparationError::Invalid("Buyurtma tanlanmagan")));
    }
    store(&state)?
        .production_journal_order(
            &actor.ref_,
            query.order_id.trim(),
            query.offset,
            query.limit.unwrap_or(30),
        )
        .await
        .map_err(error)?
        .map(Json)
        .ok_or_else(not_found)
}

pub async fn production_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ProductionJournalQuery>,
) -> Result<Json<Value>, ApiError> {
    let actor = authorize(&state, &headers).await?;
    query.validate()?;
    if query.batch_id.trim().is_empty() {
        return Err(error(PreparationError::Invalid(
            "Ishlab chiqarish yozuvi tanlanmagan",
        )));
    }
    store(&state)?
        .production_journal_batch(
            &actor.ref_,
            query.batch_id.trim(),
            query.offset,
            query.limit.unwrap_or(50),
        )
        .await
        .map_err(error)?
        .map(Json)
        .ok_or_else(not_found)
}

fn not_found() -> ApiError {
    (
        StatusCode::NOT_FOUND,
        Json(
            json!({"code":"preparation_production_not_found","error":"Ishlab chiqarish yozuvi topilmadi"}),
        ),
    )
}
