use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WsTopicKind {
    Task,
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TopicId(pub i64);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WsTopic {
    pub kind: WsTopicKind,
    pub id: TopicId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Subscribe {
        topics: Vec<WsTopic>,
        #[serde(skip_serializing_if = "Option::is_none")]
        since: Option<DateTime<Utc>>,
    },
    Unsubscribe {
        topics: Vec<WsTopic>,
    },
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Subscribed {
        topics: Vec<WsTopic>,
    },
    Unsubscribed {
        topics: Vec<WsTopic>,
    },
    Event {
        topic: WsTopic,
        event_type: String,
        timestamp: DateTime<Utc>,
        payload: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        html: Option<String>,
    },
    EventsReplay {
        events: Vec<ServerMessage>,
        timestamp: DateTime<Utc>,
    },
    Pong,
    Error {
        message: String,
    },
}

impl ServerMessage {
    pub fn timestamp(&self) -> Option<DateTime<Utc>> {
        match self {
            ServerMessage::Event { timestamp, .. } => Some(*timestamp),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example_topic() -> WsTopic {
        WsTopic {
            kind: WsTopicKind::Task,
            id: TopicId(42),
        }
    }

    #[test]
    fn test_subscribe_round_trip() {
        let msg = ClientMessage::Subscribe {
            topics: vec![example_topic()],
            since: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: ClientMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(json.contains("\"type\":\"subscribe\""), true);
        match deserialized {
            ClientMessage::Subscribe { topics, since } => {
                assert_eq!(topics.len(), 1);
                assert_eq!(topics[0].kind, WsTopicKind::Task);
                assert!(since.is_none());
            }
            _ => panic!("expected Subscribe"),
        }
    }

    #[test]
    fn test_subscribe_with_since_round_trip() {
        let since = Utc::now();
        let msg = ClientMessage::Subscribe {
            topics: vec![example_topic()],
            since: Some(since),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: ClientMessage = serde_json::from_str(&json).unwrap();
        match deserialized {
            ClientMessage::Subscribe { topics, since } => {
                assert_eq!(topics.len(), 1);
                assert!(since.is_some());
            }
            _ => panic!("expected Subscribe"),
        }
    }

    #[test]
    fn test_unsubscribe_round_trip() {
        let msg = ClientMessage::Unsubscribe {
            topics: vec![example_topic()],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"unsubscribe\""));
        let deserialized: ClientMessage = serde_json::from_str(&json).unwrap();
        match deserialized {
            ClientMessage::Unsubscribe { topics } => {
                assert_eq!(topics.len(), 1);
            }
            _ => panic!("expected Unsubscribe"),
        }
    }

    #[test]
    fn test_ping_round_trip() {
        let msg = ClientMessage::Ping;
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"ping"}"#);
        let deserialized: ClientMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, ClientMessage::Ping));
    }

    #[test]
    fn test_subscribed_round_trip() {
        let msg = ServerMessage::Subscribed {
            topics: vec![example_topic()],
        };
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: ServerMessage = serde_json::from_str(&json).unwrap();
        match deserialized {
            ServerMessage::Subscribed { topics } => {
                assert_eq!(topics.len(), 1);
            }
            _ => panic!("expected Subscribed"),
        }
    }

    #[test]
    fn test_event_round_trip() {
        let msg = ServerMessage::Event {
            topic: example_topic(),
            event_type: "agent-run-updated".to_string(),
            timestamp: Utc::now(),
            payload: serde_json::json!({"status": "running", "progress": 0.5}),
            html: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"event\""));
        assert!(json.contains("agent-run-updated"));
        assert!(json.contains("running"));
        let deserialized: ServerMessage = serde_json::from_str(&json).unwrap();
        match deserialized {
            ServerMessage::Event {
                topic,
                event_type,
                payload,
                ..
            } => {
                assert_eq!(event_type, "agent-run-updated");
                assert_eq!(topic.kind, WsTopicKind::Task);
                assert_eq!(payload["status"], "running");
            }
            _ => panic!("expected Event"),
        }
    }

    #[test]
    fn test_events_replay_round_trip() {
        let inner = ServerMessage::Event {
            topic: example_topic(),
            event_type: "test".to_string(),
            timestamp: Utc::now(),
            payload: serde_json::json!({"key": "value"}),
            html: None,
        };
        let msg = ServerMessage::EventsReplay {
            events: vec![inner],
            timestamp: Utc::now(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"events_replay\""));
        let deserialized: ServerMessage = serde_json::from_str(&json).unwrap();
        match deserialized {
            ServerMessage::EventsReplay { events, .. } => {
                assert_eq!(events.len(), 1);
                match &events[0] {
                    ServerMessage::Event { event_type, .. } => {
                        assert_eq!(event_type, "test");
                    }
                    _ => panic!("expected inner Event"),
                }
            }
            _ => panic!("expected EventsReplay"),
        }
    }

    #[test]
    fn test_pong_round_trip() {
        let msg = ServerMessage::Pong;
        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"type":"pong"}"#);
        let deserialized: ServerMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, ServerMessage::Pong));
    }

    #[test]
    fn test_error_round_trip() {
        let msg = ServerMessage::Error {
            message: "something went wrong".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"error\""));
        let deserialized: ServerMessage = serde_json::from_str(&json).unwrap();
        match deserialized {
            ServerMessage::Error { message } => {
                assert_eq!(message, "something went wrong");
            }
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn test_server_message_timestamp() {
        let now = Utc::now();
        let event = ServerMessage::Event {
            topic: example_topic(),
            event_type: "test".to_string(),
            timestamp: now,
            payload: serde_json::json!({}),
            html: None,
        };
        assert_eq!(event.timestamp(), Some(now));

        let pong = ServerMessage::Pong;
        assert_eq!(pong.timestamp(), None);
    }
}
