use serde::{Deserialize, Serialize};

use super::super::errors::ProductionMapError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductionOrderLifecycleStatus {
    Released,
    InProgress,
    ProductionCompleted,
    Closed,
    Cancelled,
}

impl Default for ProductionOrderLifecycleStatus {
    fn default() -> Self {
        Self::Released
    }
}

impl ProductionOrderLifecycleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Released => "released",
            Self::InProgress => "in_progress",
            Self::ProductionCompleted => "production_completed",
            Self::Closed => "closed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_terminal_for_material_assignment(self) -> bool {
        matches!(
            self,
            Self::ProductionCompleted | Self::Closed | Self::Cancelled
        )
    }

    pub fn can_automatically_transition_to(self, next: Self) -> bool {
        match self {
            Self::Released => matches!(
                next,
                Self::Released | Self::InProgress | Self::ProductionCompleted
            ),
            Self::InProgress => matches!(next, Self::InProgress | Self::ProductionCompleted),
            Self::ProductionCompleted | Self::Closed | Self::Cancelled => next == self,
        }
    }

    pub fn parse(value: &str) -> Result<Self, ProductionMapError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "released" => Ok(Self::Released),
            "in_progress" => Ok(Self::InProgress),
            "production_completed" => Ok(Self::ProductionCompleted),
            "closed" => Ok(Self::Closed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(ProductionMapError::StoreFailed),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionOrderLifecycleRecord {
    pub order_id: String,
    pub status: ProductionOrderLifecycleStatus,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub completion_outcome: String,
    pub lifecycle_changed_at_unix: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub production_completed_at_unix: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at_unix: Option<i64>,
    pub lifecycle_version: i64,
}

impl ProductionOrderLifecycleRecord {
    pub fn released(order_id: &str) -> Self {
        Self {
            order_id: order_id.trim().to_string(),
            status: ProductionOrderLifecycleStatus::Released,
            completion_outcome: String::new(),
            lifecycle_changed_at_unix: 0,
            production_completed_at_unix: None,
            closed_at_unix: None,
            lifecycle_version: 0,
        }
    }

    pub fn transition_to(&mut self, status: ProductionOrderLifecycleStatus, changed_at_unix: i64) {
        if self.status == status {
            return;
        }
        self.status = status;
        self.lifecycle_changed_at_unix = changed_at_unix;
        self.lifecycle_version += 1;
        if status == ProductionOrderLifecycleStatus::ProductionCompleted
            && self.production_completed_at_unix.is_none()
        {
            self.production_completed_at_unix = Some(changed_at_unix);
        }
        if status == ProductionOrderLifecycleStatus::Closed && self.closed_at_unix.is_none() {
            self.closed_at_unix = Some(changed_at_unix);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProductionOrderLifecycleStatus;

    #[test]
    fn automatic_lifecycle_transitions_never_regress_or_leave_terminal_states() {
        use ProductionOrderLifecycleStatus::{
            Cancelled, Closed, InProgress, ProductionCompleted, Released,
        };

        assert!(Released.can_automatically_transition_to(InProgress));
        assert!(Released.can_automatically_transition_to(ProductionCompleted));
        assert!(!InProgress.can_automatically_transition_to(Released));
        assert!(InProgress.can_automatically_transition_to(ProductionCompleted));
        assert!(!ProductionCompleted.can_automatically_transition_to(InProgress));
        assert!(!Closed.can_automatically_transition_to(ProductionCompleted));
        assert!(!Cancelled.can_automatically_transition_to(Released));
    }
}
