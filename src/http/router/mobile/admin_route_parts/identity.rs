fn identity_routes() -> Router<AppState> {
    Router::new()
        .route("/v1/mobile/admin/settings", any(admin::settings))
        .route(
            "/v1/mobile/admin/telegram/settings",
            any(admin::telegram_settings),
        )
        .route(
            "/v1/mobile/admin/telegram/invites",
            any(admin::telegram_invite),
        )
        .route("/v1/mobile/admin/capabilities", any(admin::capabilities))
        .route("/v1/mobile/admin/roles", any(admin::roles))
        .route("/v1/mobile/admin/workers", any(admin::workers))
        .route(
            "/v1/mobile/admin/workers/delete-check",
            any(admin::worker_delete_check),
        )
        .route("/v1/mobile/admin/system-users", any(admin::system_users))
        .route(
            "/v1/mobile/admin/system-users/detail",
            any(admin::system_user_detail),
        )
        .route(
            "/v1/mobile/admin/system-users/code/regenerate",
            any(admin::system_user_code_regenerate),
        )
        .route("/v1/mobile/admin/workers/detail", any(admin::worker_detail))
        .route(
            "/v1/mobile/admin/workers/profile-detail",
            any(admin::worker_profile_detail),
        )
        .route(
            "/v1/mobile/admin/workers/code/regenerate",
            any(admin::worker_code_regenerate),
        )
        .route("/v1/mobile/admin/worker-groups", any(admin::worker_groups))
}
