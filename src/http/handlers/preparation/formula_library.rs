use super::*;
use crate::{
    db::postgres_preparation::formula_live::{FormulaCursor, FormulaLive},
    http::{
        router::LiveSession,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
};
use axum::{
    Extension,
    response::{IntoResponse, Response},
};
use std::time::Duration;
use tokio::time::timeout;

const IO_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) async fn snapshot(state: &AppState, owner: &str) -> Result<Json<Value>, ApiError> {
    // Sample before the transaction. A commit that races the read may be
    // sent again as a patch, but can never be skipped by the client cursor.
    let cursor = state
        .preparation_formula_live
        .as_ref()
        .map(FormulaLive::cursor);
    let mut data = store(state)?
        .formula_library_rows(owner, None)
        .await
        .map_err(error)?;
    data["cursor"] = json!(cursor);
    Ok(Json(data))
}

pub(crate) async fn formula_library_live(
    State(state): State<AppState>,
    Query(cursor): Query<FormulaCursor>,
    Extension(session): Extension<LiveSession>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let principal = require_preparation_access(&state, session.principal.clone()).await?;
    let store = store(&state)?.clone();
    let live = state.preparation_formula_live.clone().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error":"preparation_formula_live_unavailable"})),
    ))?;
    let ws = session
        .protect(ws, &state, |state, actor| async move {
            require_preparation_access(&state, actor).await.is_ok()
        })
        .with_denial_notices(
            Some(r#"{"type":"error","status":401}"#),
            Some(r#"{"type":"error","status":403}"#),
        );
    Ok(ws
        .on_upgrade(move |socket| async move {
            stream(socket, store, live, principal.ref_, cursor).await;
        })
        .into_response())
}

async fn stream(
    mut socket: WebSocket,
    store: crate::db::postgres_preparation::PostgresPreparationStore,
    live: FormulaLive,
    owner: String,
    mut sent: FormulaCursor,
) {
    let mut wake = live.subscribe();
    let mut scanned = sent.clone();
    let mut first = true;
    loop {
        wake.borrow_and_update();
        if let Some(update) = live.since(&owner, Some(&scanned)) {
            let cursor = update.cursor;
            // Advance the local scan immediately, but avoid sending a frame
            // for each unrelated write. Periodic checkpoints keep resumes
            // within the bounded history even for quiet accounts.
            let frame = match update.products {
                Some(products) if products.is_empty() => (first
                    || cursor.revision.saturating_sub(sent.revision) >= 128)
                    .then(|| json!({"type":"checkpoint","cursor":cursor})),
                products => {
                    let read = store.formula_library_rows(&owner, products.as_deref());
                    let Ok(Ok(mut frame)) = timeout(IO_TIMEOUT, read).await else {
                        break;
                    };
                    if let Some(products) = products {
                        frame["type"] = "delta".into();
                        frame["base_revision"] = sent.revision.into();
                        frame["products"] = json!(products);
                    } else {
                        frame["type"] = "snapshot".into();
                    }
                    frame["cursor"] = json!(cursor);
                    Some(frame)
                }
            };
            if let Some(frame) = frame {
                let message =
                    Message::Text(crate::http::json_research::value_to_string(&frame).into());
                if !timeout(IO_TIMEOUT, socket.send(message))
                    .await
                    .is_ok_and(|r| r.is_ok())
                {
                    break;
                }
                sent = cursor.clone();
                first = false;
            }
            scanned = cursor;
        }
        tokio::select! {
            changed = wake.changed() => { if changed.is_err() { break; } }
            message = socket.recv() => match message {
                Some(Ok(Message::Ping(bytes))) => {
                    if !timeout(IO_TIMEOUT, socket.send(Message::Pong(bytes))).await.is_ok_and(|r| r.is_ok()) {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                _ => {}
            }
        }
    }
    let _ = timeout(IO_TIMEOUT, socket.send(Message::Close(None))).await;
}
