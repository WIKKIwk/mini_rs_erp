use async_trait::async_trait;
use sqlx::PgPool;

use crate::core::calculate_orders::CalculateOrderTemplate;
use crate::core::mini_orders::{
    MiniOrderError, MiniOrderSink, NewProductionOrder, OrderCreationMetadata, OrderMaterial,
    OrderMaterialTask,
};
use crate::core::production_map::{ProductionMapDefinition, ProductionMapError, QueueActionActor};
use crate::core::quantity::positive_erp_quantity;
use crate::core::werka::models::SupplierItem;

mod order_edit;

#[derive(Clone)]
pub struct PostgresMiniOrderSink {
    pool: PgPool,
}

impl PostgresMiniOrderSink {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

pub(crate) async fn save_order_creator_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    map_id: &str,
    creator: &QueueActionActor,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO mini_production_order_creators (map_id, role, principal_ref, display_name)
         VALUES ($1, $2, $3, $4) ON CONFLICT (map_id) DO NOTHING",
    )
    .bind(map_id.trim())
    .bind(creator.role.trim())
    .bind(creator.ref_.trim())
    .bind(creator.display_name.trim())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn legacy_order_creator(
    pool: &PgPool,
    order_id: &str,
) -> Result<Option<QueueActionActor>, MiniOrderError> {
    // A receipt belongs to this exact order. Reusable-template owners do not.
    // Conflicting authors are ambiguous; repeated receipts by one author agree.
    let row = sqlx::query_as::<_, (String, String, String)>(
        "WITH receipt_owner AS (
             SELECT min(btrim(owner_key)) AS owner_key
             FROM mini_order_creation_receipts WHERE map_id = $1
             HAVING count(DISTINCT btrim(owner_key)) = 1
         ), actor AS (
             SELECT split_part(owner_key, ':', 1) AS role,
                    btrim(substr(owner_key, strpos(owner_key, ':') + 1)) AS principal_ref
             FROM receipt_owner WHERE strpos(owner_key, ':') > 0
         )
         SELECT actor.role, actor.principal_ref,
                COALESCE(NULLIF(btrim(directory_user.name), ''),
                         NULLIF(btrim(builtin.display_name), ''), actor.principal_ref)
         FROM actor
         LEFT JOIN mini_system_users directory_user
           ON actor.role IN ('brigader', 'sales_manager') AND directory_user.id = actor.principal_ref
         LEFT JOIN mini_auth_builtin_identities builtin
           ON actor.role = 'admin' AND builtin.principal_ref = actor.principal_ref
         WHERE actor.role IN ('admin', 'brigader', 'sales_manager')
           AND actor.principal_ref <> ''",
    )
    .bind(order_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| MiniOrderError::StoreFailed)?;
    Ok(row.map(|(role, ref_, display_name)| QueueActionActor {
        role,
        ref_,
        display_name,
    }))
}

pub(crate) async fn remember_print_side(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    template: &CalculateOrderTemplate,
    standard: bool,
) -> Result<(), sqlx::Error> {
    if let Some(key) = crate::core::mini_orders::print_side_layout_key(template) {
        sqlx::query(
            "INSERT INTO mini_order_print_side_preferences (layout_key, standard_print_side)
             VALUES ($1, $2) ON CONFLICT (layout_key) DO UPDATE
             SET standard_print_side = excluded.standard_print_side",
        )
        .bind(key).bind(standard).execute(&mut **tx).await?;
    }
    Ok(())
}

#[async_trait]
impl MiniOrderSink for PostgresMiniOrderSink {
    async fn print_side_preference(
        &self, template: &CalculateOrderTemplate,
    ) -> Result<Option<bool>, MiniOrderError> {
        let Some(key) = crate::core::mini_orders::print_side_layout_key(template) else {
            return Ok(None);
        };
        let preference = sqlx::query_scalar(
            "SELECT standard_print_side FROM mini_order_print_side_preferences WHERE layout_key = $1",
        )
        .bind(&key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| MiniOrderError::StoreFailed)?;
        if preference.is_some() || template.item_code.trim().is_empty() {
            return Ok(preference);
        }
        // Orders created before preferences can still supply an explicit side
        // when their stored calculation proves the same print layout.
        let candidates = sqlx::query_as::<_, (serde_json::Value, bool)>(
            "SELECT o.calculation_json, (m.map_json->>'standard_print_side')::boolean
             FROM mini_orders o JOIN mini_production_maps m ON o.id = m.id
             WHERE o.customer_ref = $1 AND o.product_code = $2
               AND o.calculation_json IS NOT NULL
               AND NOT m.standard_print_side_defaulted
               AND jsonb_typeof(m.map_json->'standard_print_side') = 'boolean'
             ORDER BY m.created_at DESC, m.id DESC",
        )
        .bind(template.customer_ref.trim())
        .bind(template.item_code.trim())
        .fetch_all(&self.pool)
        .await
        .map_err(|_| MiniOrderError::StoreFailed)?;
        Ok(candidates.into_iter().find_map(|(calculation, side)| {
            let stored = serde_json::from_value(calculation).ok()?;
            (crate::core::mini_orders::print_side_layout_key(&stored).as_deref() == Some(key.as_str()))
                .then_some(side)
        }))
    }

    async fn order_creation_metadata(
        &self,
        order_id: &str,
    ) -> Result<OrderCreationMetadata, MiniOrderError> {
        let row = sqlx::query_as::<_, (i64, Option<String>, Option<String>, Option<String>)>(
            "SELECT EXTRACT(EPOCH FROM map.created_at)::bigint,
                    creator.role, creator.principal_ref, creator.display_name
             FROM mini_production_maps map
             LEFT JOIN mini_production_order_creators creator ON creator.map_id = map.id
             WHERE map.id = $1",
        )
        .bind(order_id.trim())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| MiniOrderError::StoreFailed)?;
        Ok(match row {
            Some((created_at, Some(role), Some(ref_), Some(display_name))) => {
                OrderCreationMetadata {
                    created_at_unix: Some(created_at),
                    created_by: Some(QueueActionActor {
                        role,
                        ref_,
                        display_name,
                    }),
                }
            }
            Some((created_at, ..)) => OrderCreationMetadata {
                created_at_unix: Some(created_at),
                created_by: legacy_order_creator(&self.pool, order_id).await?,
            },
            None => OrderCreationMetadata::default(),
        })
    }

    async fn save_template_map_atomic(
        &self, map: &ProductionMapDefinition, template: &CalculateOrderTemplate, owner: &str,
    ) -> Result<CalculateOrderTemplate, ProductionMapError> {
        let mut tx=self.pool.begin().await.map_err(|_| ProductionMapError::StoreFailed)?;
        super::postgres_production_map::save_edited_map_tx(&mut tx,map).await?;
        let saved=super::postgres_calculate_order::upsert_template_tx(&mut tx,owner,template.clone()).await.map_err(|error| match error {
            crate::core::calculate_orders::CalculateOrderError::IdentityConflict => ProductionMapError::TemplateIdentityConflict,
            _ => ProductionMapError::StoreFailed,
        })?;
        tx.commit().await.map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(saved)
    }
    async fn order_creation_result(
        &self,
        owner: &str,
        key: &str,
    ) -> Result<Option<(String, serde_json::Value)>, ProductionMapError> {
        sqlx::query_as("SELECT fingerprint, response_json FROM mini_order_creation_receipts WHERE owner_key = $1 AND request_key = $2")
            .bind(owner).bind(key).fetch_optional(&self.pool).await.map_err(|_| ProductionMapError::StoreFailed)
    }
    async fn order_material_tasks(
        &self,
        order_ids: &[String],
        item_groups: &[String],
    ) -> Result<Vec<OrderMaterialTask>, MiniOrderError> {
        // The apparatus selects the queue window in the client. This task list
        // contains only orders with no material assignment at any stage.
        // Linking a material is independent of reserving or consuming stock.
        let groups: Vec<_> = item_groups
            .iter()
            .map(|group| group.trim().to_lowercase())
            .collect();
        let rows = sqlx::query_as::<_, (String, String, String, f64, bool)>(
            "WITH layers AS (
                SELECT product.order_id,
                       COALESCE(NULLIF(btrim(material.payload_json->>'name'), ''),
                                btrim(layer.value->>'material')) AS material_name,
                       replace(layer.value->>'micron', ',', '.')::double precision AS micron
                FROM mini_order_products product
                CROSS JOIN LATERAL jsonb_array_elements(product.layers_json) layer(value)
                LEFT JOIN mini_calculate_materials material ON material.id = layer.value->>'material_id'
                WHERE product.order_id = ANY($1)
             )
             SELECT DISTINCT layers.order_id, item.code, item.name, layers.micron,
                    EXISTS (
                        SELECT 1 FROM mini_raw_material_assignments assignment
                        WHERE assignment.order_id = layers.order_id
                    ) AS assigned
             FROM layers JOIN mini_items item
               ON lower(btrim(item.code)) = lower(layers.material_name)
               OR lower(btrim(item.name)) = lower(layers.material_name)
             WHERE lower(btrim(item.item_group)) = ANY($2)
             ORDER BY layers.order_id, item.code, layers.micron",
        )
        .bind(order_ids).bind(groups).fetch_all(&self.pool).await
        .map_err(|_| MiniOrderError::StoreFailed)?;
        Ok(rows
            .into_iter()
            .map(
                |(order_id, item_code, material, micron, assigned)| OrderMaterialTask {
                    order_id,
                    item_code,
                    material,
                    micron,
                    assigned,
                },
            )
            .collect())
    }

    async fn order_materials(&self, order_id: &str) -> Result<Vec<OrderMaterial>, MiniOrderError> {
        // Read the order's own layers once; no reusable calculation lookup or
        // catalog pagination is needed to find its material types.
        let rows = sqlx::query_as::<_, (String, String, String, String, Vec<String>)>(
            "WITH layers AS (
                SELECT layer.value,
                       COALESCE(NULLIF(btrim(material.payload_json->>'name'), ''),
                                btrim(layer.value->>'material')) AS material_name
                FROM mini_order_products product
                CROSS JOIN LATERAL jsonb_array_elements(product.layers_json) layer(value)
                LEFT JOIN mini_calculate_materials material
                  ON material.id = layer.value->>'material_id'
                WHERE product.order_id = $1
             )
             SELECT item.code, item.name, item.uom, item.item_group,
                    array_agg(DISTINCT options.micron)
             FROM layers
             JOIN mini_items item
               ON lower(btrim(item.code)) = lower(layers.material_name)
               OR lower(btrim(item.name)) = lower(layers.material_name)
             CROSS JOIN LATERAL (
                SELECT COALESCE(btrim(layers.value->>'micron'), '') AS micron
                UNION ALL
                SELECT jsonb_array_elements_text(layers.value->'alternative_microns')
             ) options
             GROUP BY item.code, item.name, item.uom, item.item_group
             ORDER BY lower(item.code), item.code",
        )
        .bind(order_id.trim())
        .fetch_all(&self.pool)
        .await
        .map_err(|_| MiniOrderError::StoreFailed)?;
        rows.into_iter()
            .map(|(code, name, uom, item_group, values)| {
                let mut microns = values
                    .iter()
                    .map(|value| {
                        value.replace(',', ".").parse::<f64>().ok()
                            .and_then(positive_erp_quantity)
                            .ok_or(MiniOrderError::StoreFailed)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                microns.sort_by(f64::total_cmp);
                microns.dedup();
                Ok(OrderMaterial {
                    item: SupplierItem {
                        code, name, uom, item_group,
                        warehouse: String::new(),
                        customer_names: Vec::new(),
                    },
                    microns,
                })
            })
            .collect()
    }

    async fn create_order_atomic(
        &self,
        order: &NewProductionOrder,
    ) -> Result<Option<CalculateOrderTemplate>, ProductionMapError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        if let Some(creation) = &order.creation {
            sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
                .bind(format!(
                    "order-creation:{}:{}",
                    order.owner_key, creation.key
                ))
                .execute(&mut *tx)
                .await
                .map_err(|_| ProductionMapError::StoreFailed)?;
            let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_order_creation_receipts WHERE owner_key = $1 AND request_key = $2)")
                .bind(&order.owner_key).bind(&creation.key).fetch_one(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
            if exists {
                return Err(ProductionMapError::DuplicateOrderNumber);
            }
        }
        super::postgres_production_map::create_order_maps_tx(
            &mut tx,
            &order.map,
            order.template_map.as_ref(),
        )
        .await?;
        save_order_creator_tx(&mut tx, &order.map.id, &order.created_by)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let quick_template = if let Some(template) = &order.quick_template {
            Some(
                super::postgres_calculate_order::upsert_template_tx(
                    &mut tx,
                    &order.owner_key,
                    template.clone(),
                )
                .await
                .map_err(|error| match error {
                    crate::core::calculate_orders::CalculateOrderError::IdentityConflict => {
                        ProductionMapError::TemplateIdentityConflict
                    }
                    _ => ProductionMapError::StoreFailed,
                })?,
            )
        } else {
            None
        };
        save_order_tx(&mut tx, &order.map, &order.template)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        if let Some(creation) = &order.creation {
            let program = crate::core::production_map::compile_map(&order.map)?;
            let response = serde_json::json!({"ok":true,"saved":{"map":order.map,"program":program},"template":quick_template});
            sqlx::query("INSERT INTO mini_order_creation_receipts(owner_key, request_key, fingerprint, map_id, response_json) VALUES ($1,$2,$3,$4,$5)")
                .bind(&order.owner_key).bind(&creation.key).bind(&creation.fingerprint).bind(&order.map.id).bind(response)
                .execute(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        }
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        Ok(quick_template)
    }

    async fn sync_orders(
        &self,
        maps: &[ProductionMapDefinition],
        templates: &[CalculateOrderTemplate],
    ) -> Result<usize, MiniOrderError> {
        let mut synced = 0;
        for map in maps
            .iter()
            .filter(|map| map.id.starts_with("zakaz-") && !map.order_number.is_empty())
        {
            if let Some(template) = templates.iter().find(|template| {
                template.source_map_id == map.id
                    || template.order_number == map.order_number
                    || template.code == map.code
            }) {
                let mut tx = self
                    .pool
                    .begin()
                    .await
                    .map_err(|_| MiniOrderError::StoreFailed)?;
                save_order_tx_inner(&mut tx, map, template, false).await?;
                tx.commit().await.map_err(|_| MiniOrderError::StoreFailed)?;
                synced += 1;
            }
        }
        Ok(synced)
    }

    async fn order_calculation(
        &self,
        order_id: &str,
    ) -> Result<CalculateOrderTemplate, crate::core::order_edit::OrderEditError> {
        let mut tx = self.pool.begin().await?;
        let source = order_edit::load_source(&mut tx, order_id).await?;
        tx.commit().await?;
        Ok(source.template)
    }

    async fn order_edit_source(
        &self,
        order_id: &str,
    ) -> Result<crate::core::order_edit::OrderEditSource, crate::core::order_edit::OrderEditError>
    {
        let mut tx = self.pool.begin().await?;
        let source = order_edit::load_source(&mut tx, order_id).await?;
        order_edit::check_eligible(&mut tx, order_id).await?;
        tx.commit().await?;
        Ok(source)
    }

    async fn save_order_edit(
        &self,
        original: &crate::core::order_edit::OrderEditSource,
        map: &ProductionMapDefinition,
        template: &CalculateOrderTemplate,
        actor: &crate::core::production_map::QueueActionActor,
    ) -> Result<crate::core::order_edit::OrderEditSource, crate::core::order_edit::OrderEditError>
    {
        order_edit::save(&self.pool, original, map, template, actor).await
    }
    fn enabled(&self) -> bool {
        true
    }

    async fn save_order(
        &self,
        map: &ProductionMapDefinition,
        template: &CalculateOrderTemplate,
    ) -> Result<(), MiniOrderError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| MiniOrderError::StoreFailed)?;
        save_order_tx(&mut tx, map, template).await?;
        tx.commit().await.map_err(|_| MiniOrderError::StoreFailed)
    }
}

pub(crate) async fn save_order_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    map: &ProductionMapDefinition,
    template: &CalculateOrderTemplate,
) -> Result<(), MiniOrderError> {
    save_order_tx_inner(tx, map, template, true).await
}

include!("postgres_mini_order_parts/save_order_tx_inner.rs");
