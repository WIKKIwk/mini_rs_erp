use std::collections::HashMap;

use serde::Serialize;

#[derive(Serialize)]
pub(super) struct FcmPayload {
    message: FcmMessage,
}

impl FcmPayload {
    pub(super) fn new(token: &str, title: &str, body: &str, data: HashMap<String, String>) -> Self {
        Self {
            message: FcmMessage {
                token: token.to_string(),
                notification: FcmNotification {
                    title: title.to_string(),
                    body: body.to_string(),
                },
                data,
                android: FcmAndroid {
                    priority: "HIGH",
                    notification: FcmAndroidNotification {
                        channel_id: "accord_updates",
                        sound: "default",
                    },
                },
            },
        }
    }
}

#[derive(Serialize)]
struct FcmMessage {
    token: String,
    notification: FcmNotification,
    data: HashMap<String, String>,
    android: FcmAndroid,
}

#[derive(Serialize)]
struct FcmNotification {
    title: String,
    body: String,
}

#[derive(Serialize)]
struct FcmAndroid {
    priority: &'static str,
    notification: FcmAndroidNotification,
}

#[derive(Serialize)]
struct FcmAndroidNotification {
    channel_id: &'static str,
    sound: &'static str,
}
