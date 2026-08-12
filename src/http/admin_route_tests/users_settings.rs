use super::*;

#[derive(Default)]
struct BatchTrackingProfileStore {
    prefs: std::sync::Mutex<BTreeMap<String, ProfilePrefs>>,
    get_calls: AtomicUsize,
    get_many_calls: AtomicUsize,
    put_many_calls: AtomicUsize,
}

impl BatchTrackingProfileStore {
    fn with_prefs(entries: impl IntoIterator<Item = (String, ProfilePrefs)>) -> Self {
        Self {
            prefs: std::sync::Mutex::new(entries.into_iter().collect()),
            ..Self::default()
        }
    }

    fn prefs(&self, key: &str) -> ProfilePrefs {
        self.prefs
            .lock()
            .expect("profile prefs")
            .get(key)
            .cloned()
            .unwrap_or_default()
    }
}

#[async_trait]
impl ProfileStorePort for BatchTrackingProfileStore {
    async fn get(&self, key: &str) -> Result<ProfilePrefs, ProfileStoreError> {
        self.get_calls.fetch_add(1, Ordering::Relaxed);
        Ok(self.prefs(key))
    }

    async fn put(&self, key: &str, prefs: ProfilePrefs) -> Result<(), ProfileStoreError> {
        self.prefs
            .lock()
            .expect("profile prefs")
            .insert(key.to_string(), prefs);
        Ok(())
    }

    async fn get_many(&self, keys: &[String]) -> Result<Vec<ProfilePrefs>, ProfileStoreError> {
        self.get_many_calls.fetch_add(1, Ordering::Relaxed);
        let prefs = self.prefs.lock().expect("profile prefs");
        Ok(keys
            .iter()
            .map(|key| prefs.get(key).cloned().unwrap_or_default())
            .collect())
    }

    async fn put_many(&self, entries: &[(String, ProfilePrefs)]) -> Result<(), ProfileStoreError> {
        self.put_many_calls.fetch_add(1, Ordering::Relaxed);
        let mut prefs = self.prefs.lock().expect("profile prefs");
        for (key, value) in entries {
            prefs.insert(key.clone(), value.clone());
        }
        Ok(())
    }
}

struct FailingBatchProfileStore {
    prefs: BTreeMap<String, ProfilePrefs>,
    failed_key: String,
    get_calls: AtomicUsize,
    active: AtomicUsize,
    max_active: AtomicUsize,
}

#[async_trait]
impl ProfileStorePort for FailingBatchProfileStore {
    async fn get(&self, key: &str) -> Result<ProfilePrefs, ProfileStoreError> {
        self.get_calls.fetch_add(1, Ordering::Relaxed);
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(5)).await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        if key == self.failed_key {
            return Err(ProfileStoreError::StoreFailed);
        }
        Ok(self.prefs.get(key).cloned().unwrap_or_default())
    }

    async fn put(&self, _key: &str, _prefs: ProfilePrefs) -> Result<(), ProfileStoreError> {
        Ok(())
    }

    async fn get_many(&self, _keys: &[String]) -> Result<Vec<ProfilePrefs>, ProfileStoreError> {
        Err(ProfileStoreError::StoreFailed)
    }
}

struct CountingAdminStatePort {
    states: BTreeMap<String, AdminState>,
    calls: AtomicUsize,
}

#[async_trait]
impl AdminStatePort for CountingAdminStatePort {
    async fn states(&self) -> Result<BTreeMap<String, AdminState>, AdminPortError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(self.states.clone())
    }

    async fn put_state(&self, _ref_: &str, _state: AdminState) -> Result<(), AdminPortError> {
        Ok(())
    }
}

fn avatar_prefs(url: &str) -> ProfilePrefs {
    ProfilePrefs {
        avatar_url: url.to_string(),
        ..ProfilePrefs::default()
    }
}

#[tokio::test]
async fn admin_user_list_returns_merged_paged_users_with_role_labels() {
    let mut state = test_state();
    let role_store = Arc::new(MemoryRoleDefinitionStore::new());
    role_store
        .put_role_definition(RoleDefinition {
            id: "item_creator".to_string(),
            label: "Item yaratuvchi".to_string(),
            base_role: Some(PrincipalRole::Customer),
            capability_codes: vec!["catalog.item.create".to_string()],
            system: false,
        })
        .await
        .expect("role");
    role_store
        .put_role_assignment(RoleAssignment {
            principal_role: PrincipalRole::Customer,
            principal_ref: "CUST-001".to_string(),
            role_id: "item_creator".to_string(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    state.admin = state.admin.with_role_store(role_store);
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/users/list?q=customer&limit=2&offset=0",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["items"].as_array().expect("items").len(), 1);
    assert_eq!(value["items"][0]["id"], "customer:CUST-001");
    assert_eq!(value["items"][0]["source"], "customer");
    assert_eq!(value["items"][0]["entity_ref"], "CUST-001");
    assert_eq!(value["items"][0]["name"], "Customer One");
    assert_eq!(value["items"][0]["role_label"], "Item yaratuvchi");
    assert_eq!(value["has_more"], false);
}

#[tokio::test]
async fn admin_user_list_batches_only_final_page_avatar_lookups() {
    let mut state = test_state();
    let profiles = Arc::new(BatchTrackingProfileStore::with_prefs([
        (
            "supplier:DEEP-SUP-000".to_string(),
            avatar_prefs("https://cdn.test/supplier-0.jpg"),
        ),
        (
            "supplier:DEEP-SUP-001".to_string(),
            avatar_prefs("https://cdn.test/supplier-1.jpg"),
        ),
    ]));
    state.admin = state.admin.with_profile_store(profiles.clone());
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/users/list?role=supplier&q=deep-user&limit=2&offset=0",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["items"][0]["id"], "supplier:DEEP-SUP-000");
    assert_eq!(
        value["items"][0]["avatar_url"],
        "https://cdn.test/supplier-0.jpg"
    );
    assert_eq!(value["items"][1]["id"], "supplier:DEEP-SUP-001");
    assert_eq!(
        value["items"][1]["avatar_url"],
        "https://cdn.test/supplier-1.jpg"
    );
    assert_eq!(profiles.get_many_calls.load(Ordering::Relaxed), 1);
    assert_eq!(profiles.get_calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn admin_worker_user_list_uses_one_state_snapshot_and_batch_profiles() {
    let mut state = test_state();
    for (id, name, phone) in [
        ("worker_batch_1", "Batch A", "+998901110001"),
        ("worker_batch_2", "Batch B", "+998901110002"),
    ] {
        state
            .workers
            .upsert_worker(WorkerUpsert {
                id: id.to_string(),
                name: name.to_string(),
                phone: phone.to_string(),
                level: "Master".to_string(),
            })
            .await
            .expect("worker");
    }
    let states = Arc::new(CountingAdminStatePort {
        states: BTreeMap::new(),
        calls: AtomicUsize::new(0),
    });
    let profiles = Arc::new(BatchTrackingProfileStore::with_prefs([
        (
            "aparatchi:worker_batch_1".to_string(),
            avatar_prefs("https://cdn.test/worker-1.jpg"),
        ),
        (
            "worker:worker_batch_2".to_string(),
            avatar_prefs("https://cdn.test/worker-2.jpg"),
        ),
    ]));
    state.admin = state
        .admin
        .with_state_port(states.clone())
        .with_profile_store(profiles.clone());
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/users/list?role=worker&limit=20&offset=0",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["items"].as_array().expect("items").len(), 2);
    assert_eq!(
        value["items"][0]["avatar_url"],
        "https://cdn.test/worker-1.jpg"
    );
    assert_eq!(
        value["items"][1]["avatar_url"],
        "https://cdn.test/worker-2.jpg"
    );
    assert_eq!(states.calls.load(Ordering::Relaxed), 1);
    assert_eq!(profiles.get_many_calls.load(Ordering::Relaxed), 1);
    assert_eq!(profiles.get_calls.load(Ordering::Relaxed), 0);
    assert_eq!(profiles.put_many_calls.load(Ordering::Relaxed), 1);
    assert_eq!(
        profiles.prefs("aparatchi:worker_batch_2").avatar_url,
        "https://cdn.test/worker-2.jpg"
    );
}

#[tokio::test]
async fn admin_system_user_list_preserves_blocked_state_with_one_snapshot() {
    let mut state = test_state();
    for (id, name, phone) in [
        ("qolipchi_batch_1", "Qolipchi A", "+998911110001"),
        ("qolipchi_batch_2", "Qolipchi B", "+998911110002"),
    ] {
        state
            .system_users
            .upsert_user(crate::core::system_users::SystemUserUpsert {
                id: id.to_string(),
                role: PrincipalRole::Qolipchi,
                name: name.to_string(),
                phone: phone.to_string(),
            })
            .await
            .expect("system user");
    }
    let states = Arc::new(CountingAdminStatePort {
        states: BTreeMap::from([(
            "qolipchi_batch_2".to_string(),
            AdminState {
                blocked: true,
                ..AdminState::default()
            },
        )]),
        calls: AtomicUsize::new(0),
    });
    let profiles = Arc::new(BatchTrackingProfileStore::with_prefs([
        (
            "qolipchi:qolipchi_batch_1".to_string(),
            avatar_prefs("https://cdn.test/qolipchi-1.jpg"),
        ),
        (
            "qolipchi:qolipchi_batch_2".to_string(),
            avatar_prefs("https://cdn.test/qolipchi-2.jpg"),
        ),
    ]));
    state.admin = state
        .admin
        .with_state_port(states.clone())
        .with_profile_store(profiles.clone());
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/users/list?role=qolipchi&limit=20&offset=0",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["items"].as_array().expect("items").len(), 2);
    assert_eq!(value["items"][1]["blocked"], true);
    assert_eq!(value["items"][1]["status"], "blocked");
    assert_eq!(
        value["items"][1]["avatar_url"],
        "https://cdn.test/qolipchi-2.jpg"
    );
    assert_eq!(states.calls.load(Ordering::Relaxed), 1);
    assert_eq!(profiles.get_many_calls.load(Ordering::Relaxed), 1);
    assert_eq!(profiles.get_calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn admin_user_list_isolates_profile_failures_with_bounded_parallel_fallback() {
    let mut state = test_state();
    let profiles = Arc::new(FailingBatchProfileStore {
        prefs: (0..10)
            .map(|index| {
                (
                    format!("supplier:DEEP-SUP-{index:03}"),
                    avatar_prefs(&format!("https://cdn.test/supplier-{index}.jpg")),
                )
            })
            .collect(),
        failed_key: "supplier:DEEP-SUP-003".to_string(),
        get_calls: AtomicUsize::new(0),
        active: AtomicUsize::new(0),
        max_active: AtomicUsize::new(0),
    });
    state.admin = state.admin.with_profile_store(profiles.clone());
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/users/list?role=supplier&q=deep-user&limit=10&offset=0",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    let items = value["items"].as_array().expect("items");
    assert_eq!(items.len(), 10);
    for (index, item) in items.iter().enumerate() {
        assert_eq!(item["id"], format!("supplier:DEEP-SUP-{index:03}"));
        if index == 3 {
            assert_eq!(item["avatar_url"], "");
        } else {
            assert_eq!(
                item["avatar_url"],
                format!("https://cdn.test/supplier-{index}.jpg")
            );
        }
    }
    assert_eq!(profiles.get_calls.load(Ordering::Relaxed), 10);
    let max_active = profiles.max_active.load(Ordering::Relaxed);
    assert!(
        max_active >= 2,
        "fallback should run concurrently: {max_active}"
    );
    assert!(
        max_active <= 8,
        "fallback concurrency must be bounded: {max_active}"
    );
}

#[tokio::test]
async fn admin_user_list_scans_past_removed_source_pages() {
    let mut state = test_state();
    state.admin = state
        .admin
        .with_state_port(Arc::new(FakeAdminStatePort::with_removed_refs(
            (0..51).map(|index| format!("DEEP-SUP-{index:03}")),
        )));
    let token = session(&state, PrincipalRole::Admin).await;

    let first_response = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/users/list?role=supplier&q=deep-user&limit=1&offset=0",
            &token,
        ))
        .await
        .expect("first response");
    assert_eq!(first_response.status(), StatusCode::OK);
    let first = json_body(first_response).await;
    assert_eq!(first["items"].as_array().expect("items").len(), 1);
    assert_eq!(first["items"][0]["id"], "supplier:DEEP-SUP-051");
    assert_eq!(first["has_more"], true);

    let second_response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/users/list?role=supplier&q=deep-user&limit=1&offset=1",
            &token,
        ))
        .await
        .expect("second response");
    assert_eq!(second_response.status(), StatusCode::OK);
    let second = json_body(second_response).await;
    assert_eq!(second["items"].as_array().expect("items").len(), 1);
    assert_eq!(second["items"][0]["id"], "supplier:DEEP-SUP-052");
    assert_eq!(second["has_more"], false);
}

#[tokio::test]
async fn admin_customer_user_list_scans_past_material_role_assignments() {
    let mut state = test_state();
    let role_store = Arc::new(MemoryRoleDefinitionStore::new());
    for index in 0..51 {
        role_store
            .put_role_assignment(RoleAssignment {
                principal_role: PrincipalRole::MaterialTaminotchi,
                principal_ref: format!("DEEP-CUST-{index:03}"),
                role_id: "material_taminotchi".to_string(),
                assigned_apparatus: Vec::new(),
                assigned_item_groups: Vec::new(),
            })
            .await
            .expect("assignment");
    }
    state.admin = state.admin.with_role_store(role_store);
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/users/list?role=customer&q=deep-customer&limit=1&offset=0",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["items"].as_array().expect("items").len(), 1);
    assert_eq!(value["items"][0]["id"], "customer:DEEP-CUST-051");
    assert_eq!(value["has_more"], true);
}

#[tokio::test]
async fn admin_user_list_does_not_treat_customers_as_qolipchi() {
    let mut state = test_state();
    let role_store = Arc::new(MemoryRoleDefinitionStore::new());
    role_store
        .put_role_assignment(RoleAssignment {
            principal_role: PrincipalRole::Qolipchi,
            principal_ref: "CUST-001".to_string(),
            role_id: "qolipchi".to_string(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("assignment");
    state.admin = state
        .admin
        .with_role_store(role_store)
        .with_read_port(Arc::new(QolipchiCustomerLookupReadPort));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/users/list?q=qolipchi&limit=10&offset=0",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["items"].as_array().expect("items").len(), 0);
}

#[tokio::test]
async fn admin_user_list_filters_material_taminotchi_from_material_directory() {
    let mut state = test_state();
    let role_store = Arc::new(MemoryRoleDefinitionStore::new());
    role_store
        .put_role_assignment(RoleAssignment {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "MAT-NEW".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: vec!["rulon".to_string()],
        })
        .await
        .expect("assignment");
    state.admin = state.admin.with_role_store(role_store);
    let token = session(&state, PrincipalRole::Admin).await;

    let material_response = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/users/list?role=material_taminotchi&limit=10&offset=0",
            &token,
        ))
        .await
        .expect("material response");
    assert_eq!(material_response.status(), StatusCode::OK);
    let material = json_body(material_response).await;
    assert_eq!(material["items"].as_array().expect("items").len(), 1);
    assert_eq!(material["items"][0]["id"], "material_taminotchi:MAT-NEW");
    assert_eq!(material["items"][0]["source"], "material_taminotchi");
    assert_eq!(material["items"][0]["entity_ref"], "MAT-NEW");
    assert_eq!(
        material["items"][0]["principal_role"],
        "material_taminotchi"
    );
    assert_eq!(material["items"][0]["role_label"], "Material taminotchisi");

    let customer_response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/users/list?role=customer&limit=10&offset=0",
            &token,
        ))
        .await
        .expect("customer response");
    assert_eq!(customer_response.status(), StatusCode::OK);
    let customer = json_body(customer_response).await;
    assert!(
        customer["items"]
            .as_array()
            .expect("items")
            .iter()
            .any(|item| item["entity_ref"] == "CUST-001")
    );
}

#[tokio::test]
async fn admin_material_taminotchi_detail_returns_material_profile() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/material-taminotchilar/detail?ref=MAT-NEW",
            &token,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let value = json_body(response).await;
    assert_eq!(value["ref"], "MAT-NEW");
    assert_eq!(value["name"], "Materialchi");
    assert_eq!(value["phone"], "+998110000070");
    assert_eq!(value["assigned_items"].as_array().expect("items").len(), 0);
}

#[tokio::test]
async fn admin_material_taminotchi_phone_and_code_management_use_material_routes() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let phone_response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/material-taminotchilar/phone?ref=MAT-NEW",
            &token,
            r#"{"phone":"+998901234567"}"#,
        ))
        .await
        .expect("phone response");

    assert_eq!(phone_response.status(), StatusCode::OK);
    let phone_value = json_body(phone_response).await;
    assert_eq!(phone_value["ref"], "MAT-NEW");
    assert_eq!(phone_value["phone"], "+998901234567");

    let code_response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/admin/material-taminotchilar/code/regenerate?ref=MAT-NEW",
            &token,
        ))
        .await
        .expect("code response");

    assert_eq!(code_response.status(), StatusCode::OK);
    let code_value = json_body(code_response).await;
    let code = code_value["code"].as_str().expect("code");
    assert!(code.starts_with("70"), "code should use 70 prefix: {code}");
}

#[tokio::test]
async fn admin_settings_requires_admin_like_go() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Supplier).await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/admin/settings", &token))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn admin_method_checks_happen_after_auth_like_go() {
    let state = test_state();
    let cases = [
        ("PATCH", "/v1/mobile/admin/settings"),
        ("POST", "/v1/mobile/admin/roles"),
        ("PATCH", "/v1/mobile/admin/workers"),
        ("POST", "/v1/mobile/admin/production-maps"),
        ("POST", "/v1/mobile/admin/role-assignments"),
        ("PATCH", "/v1/mobile/admin/suppliers"),
        ("POST", "/v1/mobile/admin/users/list"),
        ("POST", "/v1/mobile/admin/suppliers/list"),
        ("POST", "/v1/mobile/admin/suppliers/summary"),
        ("POST", "/v1/mobile/admin/suppliers/detail"),
        ("POST", "/v1/mobile/admin/suppliers/inactive"),
        ("POST", "/v1/mobile/admin/suppliers/items/assigned"),
        ("POST", "/v1/mobile/admin/suppliers/status"),
        ("POST", "/v1/mobile/admin/suppliers/phone"),
        ("POST", "/v1/mobile/admin/suppliers/items"),
        ("GET", "/v1/mobile/admin/suppliers/items/add"),
        ("GET", "/v1/mobile/admin/suppliers/items/remove"),
        ("GET", "/v1/mobile/admin/suppliers/code/regenerate"),
        ("GET", "/v1/mobile/admin/suppliers/remove"),
        ("GET", "/v1/mobile/admin/suppliers/restore"),
        ("PATCH", "/v1/mobile/admin/customers"),
        ("POST", "/v1/mobile/admin/material-taminotchilar/detail"),
        ("POST", "/v1/mobile/admin/material-taminotchilar/phone"),
        (
            "GET",
            "/v1/mobile/admin/material-taminotchilar/code/regenerate",
        ),
        ("POST", "/v1/mobile/admin/customers/list"),
        ("POST", "/v1/mobile/admin/customers/detail"),
        ("POST", "/v1/mobile/admin/customers/phone"),
        ("GET", "/v1/mobile/admin/customers/code/regenerate"),
        ("GET", "/v1/mobile/admin/customers/items/add"),
        ("GET", "/v1/mobile/admin/customers/items/remove"),
        ("GET", "/v1/mobile/admin/customers/remove"),
        ("PATCH", "/v1/mobile/admin/items"),
        ("GET", "/v1/mobile/admin/items/bulk-move-group"),
        ("PATCH", "/v1/mobile/admin/item-groups"),
        ("POST", "/v1/mobile/admin/item-groups/tree"),
        ("POST", "/v1/mobile/admin/activity"),
        ("GET", "/v1/mobile/admin/werka/code/regenerate"),
    ];

    let supplier_token = session(&state, PrincipalRole::Supplier).await;
    let admin_token = session(&state, PrincipalRole::Admin).await;
    for (method, path) in cases {
        let unauthorized = build_router(state.clone())
            .oneshot(request(method, path, ""))
            .await
            .expect("response");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(json_body(unauthorized).await["error"], "unauthorized");

        let forbidden = build_router(state.clone())
            .oneshot(request(method, path, &supplier_token))
            .await
            .expect("response");
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN, "{path}");
        assert_eq!(json_body(forbidden).await["error"], "forbidden");

        let method_not_allowed = build_router(state.clone())
            .oneshot(request(method, path, &admin_token))
            .await
            .expect("response");
        assert_eq!(
            method_not_allowed.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{path}"
        );
        assert_eq!(
            json_body(method_not_allowed).await["error"],
            "method not allowed"
        );
    }
}
