use super::super::*;

pub(crate) struct FakeAdminStatePort {
    states: Mutex<BTreeMap<String, AdminState>>,
}

impl FakeAdminStatePort {
    pub(crate) fn new() -> Self {
        Self {
            states: Mutex::new(BTreeMap::from([
                (
                    "SUP-001".to_string(),
                    AdminState {
                        custom_code: "10CUSTOM".to_string(),
                        assigned_item_codes: vec!["ITEM-001".to_string(), "ITEM-002".to_string()],
                        ..AdminState::default()
                    },
                ),
                (
                    "SUP-002".to_string(),
                    AdminState {
                        blocked: true,
                        ..AdminState::default()
                    },
                ),
                (
                    "SUP-003".to_string(),
                    AdminState {
                        removed: true,
                        ..AdminState::default()
                    },
                ),
                (
                    "worker_001".to_string(),
                    AdminState {
                        custom_code: "401234567890".to_string(),
                        ..AdminState::default()
                    },
                ),
            ])),
        }
    }
}

#[async_trait]
impl AdminStatePort for FakeAdminStatePort {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        Ok(self.states.lock().await.clone())
    }

    async fn put_state(&self, ref_: &str, state: AdminState) -> Result<(), AdminPortError> {
        self.states.lock().await.insert(ref_.to_string(), state);
        Ok(())
    }
}

#[async_trait]
impl AdminAccessStateLookup for FakeAdminStatePort {
    async fn list_states(&self) -> Result<BTreeMap<String, AdminAccessState>, AuthPortError> {
        Ok(self
            .states
            .lock()
            .await
            .iter()
            .map(|(key, state)| {
                (
                    key.clone(),
                    AdminAccessState {
                        custom_code: state.custom_code.clone(),
                        blocked: state.blocked,
                        removed: state.removed,
                    },
                )
            })
            .collect())
    }
}

pub(crate) struct FailingAdminStatePort;

#[async_trait]
impl AdminStatePort for FailingAdminStatePort {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }

    async fn put_state(&self, _ref_: &str, _state: AdminState) -> Result<(), AdminPortError> {
        Err(AdminPortError::LookupFailed)
    }
}

pub(crate) struct LockedCustomerStatePort;

#[async_trait]
impl AdminStatePort for LockedCustomerStatePort {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        Ok(BTreeMap::from([(
            "CUST-001".to_string(),
            AdminState {
                custom_code: "30LOCKED".to_string(),
                cooldown_until: Some(time::OffsetDateTime::now_utc() + time::Duration::hours(1)),
                ..AdminState::default()
            },
        )]))
    }

    async fn put_state(&self, _ref_: &str, _state: AdminState) -> Result<(), AdminPortError> {
        Ok(())
    }
}
