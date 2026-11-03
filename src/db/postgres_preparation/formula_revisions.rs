use super::*;

pub(super) async fn catalog_recipe(
    tx: &mut Transaction<'_, Postgres>,
    variant: &str,
) -> Result<Value, PreparationError> {
    Ok(sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object(
             'set_id',set_id,'formula_code',formula_code,'name',name,
             'share_percent',share_percent::text,'lines',lines,'updated_at',updated_at,
             'variant_id',variant_id) ORDER BY lower(name),name),'[]'::jsonb)
         FROM mini_preparation_formulas WHERE variant_id=$1",
    )
    .bind(variant)
    .fetch_one(&mut **tx)
    .await?)
}

pub(super) async fn recipe_revision(
    tx: &mut Transaction<'_, Postgres>,
    variant: &str,
    formulas: &Value,
) -> Result<i64, PreparationError> {
    // Formula writers and order selection share the owner's transaction lock.
    let existing = sqlx::query_scalar(
        "SELECT id FROM mini_preparation_formula_revisions
         WHERE variant_id=$1 AND content_key=public.mini_preparation_recipe_key($2)",
    )
    .bind(variant)
    .bind(formulas)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(id) = existing {
        return Ok(id);
    }
    Ok(sqlx::query_scalar(
        "INSERT INTO mini_preparation_formula_revisions(variant_id,content_key,formulas)
         VALUES ($1,public.mini_preparation_recipe_key($2),$2) RETURNING id",
    )
    .bind(variant)
    .bind(formulas)
    .fetch_one(&mut **tx)
    .await?)
}

pub(super) struct OrderRecipe {
    pub name: String,
    pub revision: i64,
    pub formulas: Value,
    pub is_pinned: bool,
}

impl OrderRecipe {
    pub fn summary(&self, variant: &str, code: &str) -> Value {
        let formulas = self.formulas.as_array().expect("recipe formulas array");
        let shares = formulas.iter().try_fold(0, |total, formula| {
            Some(total + decimal(formula["share_percent"].as_str()?).ok()?)
        });
        json!({
            "id":variant,"name":self.name,"formula_code":code,
            "revision_id":self.revision,"formula_count":formulas.len(),
            "shares_complete":!formulas.is_empty() && shares==Some(100*SCALE)
        })
    }
}

pub(super) async fn selected_recipe(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    order: &str,
    variant: &str,
) -> Result<Option<OrderRecipe>, PreparationError> {
    let row: Option<(String, i64, Value, bool)> = sqlx::query_as(
        "SELECT s.display_name, r.id, r.formulas, s.is_pinned
         FROM mini_preparation_order_formula_selections s
         JOIN mini_preparation_formula_revisions r ON r.id=s.revision_id
         WHERE s.owner_ref=$1 AND s.order_id=$2 AND s.variant_id=$3",
    )
    .bind(owner)
    .bind(order)
    .bind(variant)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(
        row.map(|(name, revision, formulas, is_pinned)| OrderRecipe {
            name,
            revision,
            formulas,
            is_pinned,
        }),
    )
}

pub(super) async fn pin_order_recipe(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    order: &str,
    variant: &str,
) -> Result<OrderRecipe, PreparationError> {
    let existing = selected_recipe(tx, owner, order, variant).await?;
    if let Some(selection) = existing.filter(|selection| selection.is_pinned) {
        return Ok(selection);
    }
    let (name, product, material): (String, String, String) = sqlx::query_as(
        "SELECT name,product_code,material_id FROM mini_preparation_formula_variants
         WHERE id=$1 AND owner_ref=$2",
    )
    .bind(variant)
    .bind(owner)
    .fetch_one(&mut **tx)
    .await?;
    let formulas = catalog_recipe(tx, variant).await?;
    let revision = recipe_revision(tx, variant, &formulas).await?;
    // An empty catalog has no recipe to freeze. Retain the order name while
    // the operator fills in the reusable draft, then pin on an explicit selection.
    let is_pinned = formulas.as_array().is_some_and(|parts| !parts.is_empty());
    sqlx::query(
        "INSERT INTO mini_preparation_order_formula_selections
         (owner_ref,order_id,variant_id,product_code,material_id,revision_id,display_name,is_pinned)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
         ON CONFLICT (owner_ref,order_id,variant_id)
         DO UPDATE SET revision_id=EXCLUDED.revision_id,is_pinned=EXCLUDED.is_pinned
         WHERE NOT mini_preparation_order_formula_selections.is_pinned",
    )
    .bind(owner)
    .bind(order)
    .bind(variant)
    .bind(product)
    .bind(material)
    .bind(revision)
    .bind(&name)
    .bind(is_pinned)
    .execute(&mut **tx)
    .await?;
    // Read the canonical revision when the same composition was saved before.
    selected_recipe(tx, owner, order, variant)
        .await?
        .ok_or(PreparationError::StoreFailed)
}

pub(super) async fn update_order_revision(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    order: &str,
    variant: &str,
    formulas: &Value,
) -> Result<(), PreparationError> {
    let revision = recipe_revision(tx, variant, formulas).await?;
    sqlx::query(
        "UPDATE mini_preparation_order_formula_selections SET revision_id=$4
         WHERE owner_ref=$1 AND order_id=$2 AND variant_id=$3 AND revision_id<>$4",
    )
    .bind(owner)
    .bind(order)
    .bind(variant)
    .bind(revision)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
