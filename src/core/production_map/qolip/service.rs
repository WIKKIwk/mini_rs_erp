use super::*;

impl ProductionMapService {
    pub async fn qolip_codes_for_resume(
        &self,
        apparatus: &str,
        order_id: &str,
    ) -> Result<Vec<String>, ProductionMapError> {
        Ok(self
            .store
            .active_order_run_session(apparatus, order_id)
            .await?
            .and_then(|session| super::progress::QolipLineage::from_payload(&session.payload_json))
            .map(|lineage| lineage.qolip_codes)
            .unwrap_or_default())
    }

    pub async fn active_order_run_session_for_qolip(
        &self,
        qolip_code: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        self.store
            .active_order_run_session_for_qolip(qolip_code)
            .await
    }

    pub async fn order_run_sessions_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        self.store.order_run_sessions_for_order(order_id).await
    }
}
