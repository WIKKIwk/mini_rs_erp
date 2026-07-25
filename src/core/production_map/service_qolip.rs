use super::*;

impl ProductionMapService {
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
