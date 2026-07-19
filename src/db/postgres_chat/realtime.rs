use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::PgListener;

use crate::core::chat::ChatHub;

const CHAT_REALTIME_CHANNEL: &str = "mini_chat_realtime";

pub fn start_realtime_listener(pool: PgPool, hub: ChatHub) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    handle.spawn(async move {
        loop {
            if let Err(error) = listen_once(&pool, &hub).await {
                tracing::warn!(%error, "chat postgres realtime listener disconnected");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    });
}

async fn listen_once(pool: &PgPool, hub: &ChatHub) -> Result<(), sqlx::Error> {
    let mut listener = PgListener::connect_with(pool).await?;
    listener.listen(CHAT_REALTIME_CHANNEL).await?;
    loop {
        let notification = listener.recv().await?;
        match super::read::outbox_event(pool, notification.payload()).await {
            Ok(event) => hub.publish(event.payload, &event.recipient_keys).await,
            Err(error) => tracing::warn!(%error, "chat realtime event load failed"),
        }
    }
}
