use crate::db::postgres::PostgresConfig;

pub(super) fn postgres_pool(component: &'static str) -> Option<sqlx::PgPool> {
    let config = match PostgresConfig::from_env() {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!(?error, component, "mini ERP postgres store unavailable");
            return None;
        }
    };
    match config.pool_options().connect_lazy(&config.database_url) {
        Ok(pool) => Some(pool),
        Err(error) => {
            tracing::warn!(%error, component, "mini ERP postgres store unavailable");
            None
        }
    }
}
