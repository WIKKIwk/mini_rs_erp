use super::{PostgresTrainingWorkspaceStore, TrainingWorkspaceError};

impl PostgresTrainingWorkspaceStore {
    pub async fn delete_order(&self, order_id: &str) -> Result<(), TrainingWorkspaceError> {
        let order_id = order_id.trim();
        if order_id.is_empty() || !order_id.starts_with("training-") {
            return Err(TrainingWorkspaceError::InvalidInput(
                "training order id kerak".to_string(),
            ));
        }
        let row = sqlx::query(
            "WITH target AS (
                 SELECT order_number
                 FROM mini_training_production_maps
                 WHERE id = $1
             ), templates AS MATERIALIZED (
                 SELECT payload_json->>'image_id' AS image_id
                 FROM mini_training_quick_order_templates
                 WHERE payload_json->>'source_map_id' = $1
                    OR payload_json->>'order_number' = (SELECT order_number FROM target)
             ), deleted_images AS (
                 DELETE FROM mini_training_order_images image
                 USING templates
                 WHERE image.image_id = templates.image_id
                 RETURNING image.image_id
             ), deleted_templates AS (
                 DELETE FROM mini_training_quick_order_templates
                 WHERE payload_json->>'source_map_id' = $1
                    OR payload_json->>'order_number' = (SELECT order_number FROM target)
                 RETURNING id
             ), deleted_assignments AS (
                 DELETE FROM mini_training_raw_material_assignments
                 WHERE order_id = $1
                 RETURNING id
             )
             DELETE FROM mini_training_production_maps
             WHERE id = $1
             RETURNING id",
        )
        .bind(order_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
        if row.is_none() {
            return Err(TrainingWorkspaceError::MapNotFound);
        }
        Ok(())
    }
}
