use serde::{Deserialize, Serialize};
use serde_json::Value;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineCommandContext {
    pub actor_key: String,
    pub idempotency_key: String,
}

impl EngineCommandContext {
    #[allow(dead_code)]
    pub fn new(actor_key: &str, idempotency_key: &str) -> Result<Self, EngineError> {
        let actor_key = actor_key.trim().to_string();
        let idempotency_key = idempotency_key.trim().to_string();
        if idempotency_key.is_empty() {
            return Err(EngineError::BlankIdempotencyKey);
        }
        Ok(Self {
            actor_key,
            idempotency_key,
        })
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineEventDraft {
    pub domain: String,
    pub action: String,
    pub entity_id: String,
    pub actor_key: String,
    pub idempotency_key: String,
    pub payload_json: Value,
}

impl EngineEventDraft {
    #[allow(dead_code)]
    pub fn new(
        context: &EngineCommandContext,
        domain: &str,
        action: &str,
        entity_id: &str,
        payload_json: Value,
    ) -> Result<Self, EngineError> {
        let domain = domain.trim().to_string();
        let action = action.trim().to_string();
        if domain.is_empty() {
            return Err(EngineError::BlankEventDomain);
        }
        if action.is_empty() {
            return Err(EngineError::BlankEventAction);
        }
        Ok(Self {
            domain,
            action,
            entity_id: entity_id.trim().to_string(),
            actor_key: context.actor_key.clone(),
            idempotency_key: context.idempotency_key.clone(),
            payload_json,
        })
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    #[error("idempotency key is required")]
    BlankIdempotencyKey,
    #[error("event domain is required")]
    BlankEventDomain,
    #[error("event action is required")]
    BlankEventAction,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_context_rejects_blank_idempotency_key() {
        let error = EngineCommandContext::new("admin:admin", " ")
            .expect_err("blank idempotency key rejected");

        assert_eq!(error, EngineError::BlankIdempotencyKey);
    }

    #[test]
    fn engine_context_trims_actor_and_idempotency_key() {
        let context = EngineCommandContext::new(" admin:admin ", " move-123 ")
            .expect("context");

        assert_eq!(context.actor_key, "admin:admin");
        assert_eq!(context.idempotency_key, "move-123");
    }

    #[test]
    fn engine_event_records_domain_and_action() {
        let context = EngineCommandContext::new("admin:admin", "order-open-1")
            .expect("context");
        let event = EngineEventDraft::new(
            &context,
            "production_maps",
            "batch_move",
            "zakaz-1001",
            serde_json::json!({"from":"7 ta rangli pechat","to":"8 ta rangli pechat"}),
        )
        .expect("event");

        assert_eq!(event.domain, "production_maps");
        assert_eq!(event.action, "batch_move");
        assert_eq!(event.entity_id, "zakaz-1001");
        assert_eq!(event.actor_key, "admin:admin");
        assert_eq!(event.idempotency_key, "order-open-1");
        assert_eq!(event.payload_json["to"], "8 ta rangli pechat");
    }

    #[test]
    fn engine_event_rejects_blank_domain_or_action() {
        let context = EngineCommandContext::new("admin:admin", "event-1")
            .expect("context");

        assert_eq!(
            EngineEventDraft::new(
                &context,
                " ",
                "batch_move",
                "zakaz-1001",
                serde_json::json!({})
            )
            .expect_err("blank domain"),
            EngineError::BlankEventDomain
        );
        assert_eq!(
            EngineEventDraft::new(
                &context,
                "production_maps",
                " ",
                "zakaz-1001",
                serde_json::json!({})
            )
            .expect_err("blank action"),
            EngineError::BlankEventAction
        );
    }
}
