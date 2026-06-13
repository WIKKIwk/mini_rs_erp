use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::core::push::ports::{PushSenderPort, PushTokenStorePort};
use crate::fcm::FcmPushSender;
use crate::store::push_token_store::PushTokenStore;

#[tokio::test]
async fn fcm_sender_sends_go_payload_and_drops_stale_token() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(PushTokenStore::new(tempdir.path().join("push_tokens.json")));
    store
        .move_token_to_key("supplier:SUP-001", "stale-token", "android")
        .await
        .expect("seed stale");
    store
        .move_token_to_key("supplier:SUP-001", "fresh-token", "android")
        .await
        .expect("seed fresh");

    let state = MockFcmState::default();
    let base_url = spawn_mock_fcm(state.clone()).await;
    let sender = FcmPushSender::new_for_tests(
        store.clone(),
        "firebase-adminsdk@example.iam.gserviceaccount.com",
        TEST_PRIVATE_KEY,
        &format!("{base_url}/token"),
        &format!("{base_url}/send"),
    );

    sender
        .send_to_key(
            "supplier:SUP-001",
            "Title",
            "Body",
            HashMap::from([("id".to_string(), "1".to_string())]),
        )
        .await
        .expect("send");

    let requests = state.requests.lock().await.clone();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.authorization == "Bearer token")
            .count(),
        2
    );
    let first_payload: Value = serde_json::from_str(&requests[0].body).expect("payload");
    assert_eq!(first_payload["message"]["token"], "stale-token");
    assert_eq!(first_payload["message"]["notification"]["title"], "Title");
    assert_eq!(first_payload["message"]["notification"]["body"], "Body");
    assert_eq!(first_payload["message"]["data"]["id"], "1");
    assert_eq!(first_payload["message"]["android"]["priority"], "HIGH");
    assert_eq!(
        first_payload["message"]["android"]["notification"]["channel_id"],
        "accord_updates"
    );
    assert_eq!(
        first_payload["message"]["android"]["notification"]["sound"],
        "default"
    );

    let remaining = store.list("supplier:SUP-001").await.expect("list");
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].token, "fresh-token");
}

async fn spawn_mock_fcm(state: MockFcmState) -> String {
    let app = Router::new()
        .route("/token", post(token_handler))
        .route("/send", post(send_handler))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    format!("http://{addr}")
}

async fn token_handler() -> Json<Value> {
    Json(json!({
        "access_token": "token",
        "expires_in": 3600
    }))
}

async fn send_handler(
    State(state): State<MockFcmState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let body = String::from_utf8_lossy(&body).to_string();
    state.requests.lock().await.push(MockFcmRequest {
        authorization,
        body: body.clone(),
    });

    if body.contains("stale-token") {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": {
                    "message": "Requested entity was not found.",
                    "status": "NOT_FOUND",
                    "details": [{ "errorCode": "UNREGISTERED" }]
                }
            })),
        )
            .into_response();
    }
    if body.contains("fresh-token") {
        return (StatusCode::OK, Json(json!({}))).into_response();
    }
    (StatusCode::BAD_REQUEST, body).into_response()
}

#[derive(Clone, Default)]
struct MockFcmState {
    requests: Arc<Mutex<Vec<MockFcmRequest>>>,
}

#[derive(Clone)]
struct MockFcmRequest {
    authorization: String,
    body: String,
}

const TEST_PRIVATE_KEY: &str = r#"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDydatENz2MLGYr
H3j+5vpEOP181WWeSAxdaFe3Upv9F/hrl0Y42Ya7GGy6j2EyOkqpUGiWhApB6S0/
0fYL8fCIhQ/sb+YlKBTpQ4eFj5epGUVr8wHBkVvyNFOQQ/lBc6shyhifbJ+oYc7I
wNvegm6d6xffWzQGNOxBsB7cKnk2q3fztYiT2CIG9XHbYy9WHNaQAv4cU4OabRkP
IryGoVhkZWLhzPunDNjj3Py5Zo7VtB8Jy3dZqXhYnSjFVTSqk3cTtq67v72ssMr/
5aMyhnrCYTRPw0erQuvm+4ExEh8gQ+v8Rp9dST4bWe8nJ0VbiMpd9tO2k/P4HbA2
UPp4sPz9AgMBAAECggEAXkx3lOtrK6ZlAiaWd4U8FuaXaELS5/GbpYScgPdHJfN5
sda/AANSTFgeiZyUL+XN/fYBB3FJUAMxjx3I9TJe26ns3IdU2mSxZVvXTJHhaWoj
vu1fZHp1aUkCqxxUyCkFiPnCA9dKbUHFG/0uRmcyQIcb3Mnq/PL+ZDnsdrKqPCFI
kZAIkyGXrkXB4sRBK+nbwW/vgUVa+nWXUBYMjHn5pST10ViYaitXAqgaQ3O9wIuC
UtT81DYzp8/BOKJM7WVnUoGi0mZ8hzpuo4nyFRREtcr8lC8TyIKVKFUgiPoBS3//
lZlWilXhZaKAcuzESV8eaxlTrII/+cfjMe88ONo0HQKBgQD8P9JxcMYAC0E00JKZ
5TqFZGPimq1k81hxGpCX15B1vf73mNEk/5YZ09wCi/7Kaoo+2Bl6mEv68Z7PZbgA
yNLKGk078oVmx2oPOlfYWCM8u3qUHZjcgqE5NQqpAG24HjcAv0DeIF1KNxcofspA
9G9/638FB1oMYKCBKqO5Zf6TtwKBgQD2EJU90CO7HlmS4RKHwow/R6Vnw9jq4UmR
Wb72FihgQN+x/6bSf50PiA2fdV4Ic2L5gq7tbUfBx1ZR+j/VCy3Py9CKx5uXgjUH
Lc9hbPrSVuQH4kM/HzUFJ0lo6p+J38doGpLqXp0Sx8nUrJbqh3cPOmriM2PXdiVS
s7YH2qO86wKBgH6E6GF7peQJwRfjcVR9NAAJ3UugN04F/BsmrtVqCovz0vmPDX+Y
LkogCB7C5vXRwCtLKmRiFOH15KizpTnHgGpcDNb/ikeFx72BjuP1OR9SDWZS/gPE
BWdzIjin/WA2z3Gxe7Ct3PzHavcluP4hW/d2P8xe5pyErpx6rYnlDW47AoGBALfq
PSIuaAZ78MdvosIGD31ct6yPHZqxOKODSM/2T8dhtdD9HFtJNsNdFZGRz+7RD7Ee
lFCx1Who7YPoX72E1YDy/bQ87XaYw7nR66cOJYsBlv6th0WutZpceuoIM6aBtDGD
azvx68UVvy1Osp4pEjw3lZvsfTuV+t+NowjLyoZxAoGBAOi84/H/yHI5ThJOQUYa
IaKn2Jj6iruXetwX7ezmi5QbOCWvzaug+CRYbC+aovF12IVEQXXoscjq8G/iaR46
gXa2m1NnuOjO9AUsrZKo3orynqywXD5kMUejOz8z4u1XxMAhkOrOIrLAm8z8GBEs
i6/4bMWNEu43nRNKCCSmu8OI
-----END PRIVATE KEY-----"#;
