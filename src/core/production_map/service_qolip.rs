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
}
