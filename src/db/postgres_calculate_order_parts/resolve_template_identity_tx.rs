async fn resolve_template_identity_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner_key: &str,
    template: &CalculateOrderTemplate,
    read_only: bool,
) -> Result<Option<crate::core::calculate_orders::TemplateIdentity>, CalculateOrderError> {
    let by_id = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT id, owner_key, item_code, coalesce(payload_json->>'variant_id','') FROM mini_quick_order_templates WHERE id = $1")
        .bind(template.id.trim()).fetch_optional(&mut **tx).await.map_err(|_| CalculateOrderError::StoreFailed)?;
    let by_code = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT id, owner_key, item_code, coalesce(payload_json->>'variant_id','') FROM mini_quick_order_templates WHERE owner_key = $1 AND lower(code) = lower($2) ORDER BY saved_at DESC LIMIT 1")
        .bind(owner_key.trim()).bind(template.code.trim()).fetch_optional(&mut **tx).await.map_err(|_| CalculateOrderError::StoreFailed)?;
    let identity =
        |(id, owner, item_code, variant_id)| crate::core::calculate_orders::TemplateIdentity {
            id,
            owner,
            item_code,
            variant_id,
        };
    if read_only {
        let existing = if template.id.trim().is_empty() { by_code } else { by_id };
        return crate::core::calculate_orders::checked_template_read_identity(owner_key,template,existing.map(identity));
    }
    return crate::core::calculate_orders::checked_template_identity(
        owner_key,
        template,
        by_id.map(identity),
        by_code.map(identity),
    );
}
