use super::formula_revisions::{
    OrderRecipe, catalog_recipe, recipe_revision, selected_recipe, update_order_revision,
};
use super::*;

pub(super) async fn scoped_selection(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    order: &str,
    product: &str,
    material: &str,
    variant: &str,
) -> Result<Option<OrderRecipe>, PreparationError> {
    if order.is_empty() {
        return Ok(None);
    }
    formula_variant_orders::order_recipe_scope(tx, owner, order, product, material, variant)
        .await?;
    selected_recipe(tx, owner, order, variant).await
}

pub(super) async fn record_catalog_revision(
    tx: &mut Transaction<'_, Postgres>,
    variant: &str,
) -> Result<(), PreparationError> {
    let formulas = catalog_recipe(tx, variant).await?;
    recipe_revision(tx, variant, &formulas).await?;
    Ok(())
}

pub(super) async fn upsert_order_component(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    order: &str,
    variant: &str,
    mut recipe: OrderRecipe,
    name: &str,
    lines: Value,
) -> Result<Value, PreparationError> {
    let formulas = recipe
        .formulas
        .as_array_mut()
        .expect("recipe formulas array");
    let formula = if let Some(existing) = formulas.iter_mut().find(|f| f["name"] == name) {
        existing["lines"] = lines;
        existing.clone()
    } else {
        let formula_code: String =
            sqlx::query_scalar("SELECT public.mini_preparation_next_formula_code()")
                .fetch_one(&mut **tx)
                .await?;
        let formula = json!({
            "formula_code":formula_code,"name":name,"lines":lines,"variant_id":variant,
            "share_percent":if formulas.is_empty() { Some("100.000000") } else { None }
        });
        formulas.push(formula.clone());
        formulas.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        formula
    };
    update_order_revision(tx, owner, order, variant, &recipe.formulas).await?;
    Ok(formula)
}

impl PostgresPreparationStore {
    pub async fn list_order_formula_components(
        &self,
        owner: &str,
        product: &str,
        material: &str,
        variant: &str,
        order: &str,
    ) -> Result<Value, PreparationError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let selection = scoped_selection(&mut tx, owner, order, product, material, variant).await?;
        let (name, formulas) = match selection {
            Some(recipe) if recipe.is_pinned => (recipe.name, recipe.formulas),
            Some(draft) => (draft.name, catalog_recipe(&mut tx, variant).await?),
            None => {
                let name = sqlx::query_scalar(
                    "SELECT name FROM mini_preparation_formula_variants WHERE id=$1",
                )
                .bind(variant)
                .fetch_one(&mut *tx)
                .await?;
                (name, catalog_recipe(&mut tx, variant).await?)
            }
        };
        let key: String =
            sqlx::query_scalar("SELECT encode(public.mini_preparation_recipe_key($1),'hex')")
                .bind(&formulas)
                .fetch_one(&mut *tx)
                .await?;
        tx.commit().await?;
        Ok(
            json!({"product_code":product,"material_id":material,"variant_name":name,
            "recipe_key":key,"formulas":formulas}),
        )
    }
}
