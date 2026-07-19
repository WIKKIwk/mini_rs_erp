use std::collections::HashMap;

use serde::Serialize;

#[derive(Serialize)]
pub(super) struct FcmPayload {
    message: FcmMessage,
}

impl FcmPayload {
    pub(super) fn new(token: &str, title: &str, body: &str, data: HashMap<String, String>) -> Self {
        let is_chat = data
            .get("event_type")
            .is_some_and(|event_type| event_type.trim() == "chat.message.created");
        let channel_id = if is_chat {
            "accord_chat"
        } else {
            "accord_updates"
        };
        let apns = is_chat.then(|| FcmApns {
            headers: FcmApnsHeaders { priority: "10" },
            payload: FcmApnsPayload {
                aps: FcmAps {
                    sound: "default",
                    thread_id: data
                        .get("conversation_id")
                        .map(|value| value.trim())
                        .filter(|value| !value.is_empty())
                        .map(str::to_string),
                },
            },
        });
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
                        channel_id,
                        sound: "default",
                    },
                },
                apns,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    apns: Option<FcmApns>,
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

#[derive(Serialize)]
struct FcmApns {
    headers: FcmApnsHeaders,
    payload: FcmApnsPayload,
}

#[derive(Serialize)]
struct FcmApnsHeaders {
    #[serde(rename = "apns-priority")]
    priority: &'static str,
}

#[derive(Serialize)]
struct FcmApnsPayload {
    aps: FcmAps,
}

#[derive(Serialize)]
struct FcmAps {
    sound: &'static str,
    #[serde(rename = "thread-id", skip_serializing_if = "Option::is_none")]
    thread_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::FcmPayload;

    #[test]
    fn chat_messages_use_the_chat_notification_channel() {
        let payload = FcmPayload::new(
            "token",
            "title",
            "body",
            HashMap::from([
                ("event_type".to_string(), "chat.message.created".to_string()),
                ("conversation_id".to_string(), "conversation-1".to_string()),
            ]),
        );

        let json = serde_json::to_value(payload).expect("serialize payload");
        assert_eq!(
            json["message"]["android"]["notification"]["channel_id"],
            "accord_chat"
        );
        assert_eq!(json["message"]["apns"]["headers"]["apns-priority"], "10");
        assert_eq!(
            json["message"]["apns"]["payload"]["aps"]["sound"],
            "default"
        );
        assert_eq!(
            json["message"]["apns"]["payload"]["aps"]["thread-id"],
            "conversation-1"
        );
    }

    #[test]
    fn non_chat_messages_keep_the_updates_notification_channel() {
        let payload = FcmPayload::new(
            "token",
            "title",
            "body",
            HashMap::from([("event_type".to_string(), "dispatch.created".to_string())]),
        );

        let json = serde_json::to_value(payload).expect("serialize payload");
        assert_eq!(
            json["message"]["android"]["notification"]["channel_id"],
            "accord_updates"
        );
        assert!(json["message"].get("apns").is_none());
    }
}
