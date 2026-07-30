use hiqlite::Client;

use crate::db::schema::Message;
use crate::providers::types::ProviderEvent;

pub async fn persist_event(
    client: &Client,
    event: &ProviderEvent,
    session_id: &str,
    project_key: i64,
) -> Result<(), hiqlite::Error> {
    let entry_json = serde_json::to_value(event)
        .map_err(|e| hiqlite::Error::new(format!("serialize event: {e}")))?;

    let timestamp = event_timestamp(event)
        .map(|ts| ts.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default();

    client
        .execute(
            "INSERT INTO messages (project_key, session_id, seq, entry_json, timestamp)
             VALUES ($1, $2,
               (SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE project_key = $1 AND session_id = $2),
               $3, $4)",
            hiqlite::params!(project_key, session_id, entry_json.to_string(), timestamp),
        )
        .await?;
    Ok(())
}

fn event_timestamp(event: &ProviderEvent) -> Option<chrono::NaiveDateTime> {
    match event {
        ProviderEvent::UserText { timestamp, .. }
        | ProviderEvent::Text { timestamp, .. }
        | ProviderEvent::ToolUse { timestamp, .. }
        | ProviderEvent::ToolResult { timestamp, .. }
        | ProviderEvent::Thinking { timestamp, .. }
        | ProviderEvent::Error { timestamp, .. }
        | ProviderEvent::Done { timestamp, .. } => Some(*timestamp),
        _ => None,
    }
}

pub async fn update_tool_event(
    client: &Client,
    tool_use_id: &str,
    merged_event: &ProviderEvent,
    session_id: &str,
    project_key: i64,
) -> Result<bool, hiqlite::Error> {
    let messages = client
        .query_map::<Message, _>(
            "SELECT project_key, session_id, seq, entry_json FROM messages WHERE project_key = $1 AND session_id = $2 ORDER BY seq ASC",
            hiqlite::params!(project_key, session_id),
        )
        .await?;

    for m in &messages {
        if let Ok(ProviderEvent::ToolUse {
            tool_use_id: Some(ref id),
            ..
        }) = serde_json::from_value::<ProviderEvent>(m.entry_json.clone())
        {
            if id == tool_use_id {
                let entry_json = serde_json::to_value(merged_event)
                    .map_err(|e| hiqlite::Error::new(format!("serialize event: {e}")))?;
                client
                    .execute(
                        "UPDATE messages SET entry_json = $1 WHERE project_key = $2 AND session_id = $3 AND seq = $4",
                        hiqlite::params!(entry_json.to_string(), project_key, session_id, m.seq),
                    )
                    .await?;
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub async fn load_transcript(
    client: &Client,
    session_id: &str,
    project_key: i64,
) -> Result<Vec<ProviderEvent>, hiqlite::Error> {
    let messages = client
        .query_map::<Message, _>(
            "SELECT project_key, session_id, seq, entry_json FROM messages WHERE project_key = $1 AND session_id = $2 ORDER BY seq ASC",
            hiqlite::params!(project_key, session_id),
        )
        .await?;

    let mut events = Vec::with_capacity(messages.len());
    for m in messages {
        match serde_json::from_value(m.entry_json) {
            Ok(event) => events.push(event),
            Err(e) => {
                tracing::warn!(
                    "load_transcript: skipping event seq={}: deserialize error: {e}",
                    m.seq
                );
            }
        }
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use chrono::NaiveDateTime;
    use tempfile::TempDir;

    fn test_ts() -> NaiveDateTime {
        NaiveDateTime::parse_from_str("2024-01-15 12:00:00", "%Y-%m-%d %H:%M:%S").unwrap()
    }

    async fn make_client() -> (hiqlite::Client, TempDir) {
        let tmp = TempDir::new().unwrap();
        let config = hiqlite::NodeConfig {
            node_id: 1,
            nodes: vec![hiqlite::Node {
                id: 1,
                addr_raft: "127.0.0.1:0".into(),
                addr_api: "127.0.0.1:0".into(),
            }],
            data_dir: tmp.path().to_str().unwrap().to_string().into(),
            secret_raft: "test-raft-secret-123".into(),
            secret_api: "test-api-secret-123".into(),
            ..Default::default()
        };
        let client = hiqlite::start_node(config).await.unwrap();
        client.wait_until_healthy_db().await;
        db::run_migrations(&client).await.unwrap();
        (client, tmp)
    }

    fn make_events() -> Vec<ProviderEvent> {
        let ts = test_ts();
        vec![
            ProviderEvent::Text {
                text: "hello".into(),
                timestamp: ts,
            },
            ProviderEvent::ToolUse {
                tool_name: "read".into(),
                tool_use_id: Some("id1".into()),
                input: serde_json::json!({"path": "/tmp"}),
                result: None,
                message_id: None,
                timestamp: ts,
            },
            ProviderEvent::ToolResult {
                tool_use_id: Some("id1".into()),
                result: "ok".into(),
                message_id: None,
                timestamp: ts,
            },
            ProviderEvent::Thinking {
                thinking: "hmm".into(),
                timestamp: ts,
            },
            ProviderEvent::Done {
                data: serde_json::json!({"status": "ok"}),
                timestamp: ts,
            },
        ]
    }

    #[tokio::test]
    async fn test_persist_and_load_transcript() {
        let (client, _tmp) = make_client().await;
        let session_id = "sess-1";
        let project_key = 1i64;
        let events = make_events();

        for event in &events {
            persist_event(&client, event, session_id, project_key)
                .await
                .unwrap();
        }

        let loaded = load_transcript(&client, session_id, project_key)
            .await
            .unwrap();

        assert_eq!(loaded.len(), events.len(), "event count mismatch");
        for (i, (orig, loaded)) in events.iter().zip(loaded.iter()).enumerate() {
            assert_eq!(orig, loaded, "event {i} mismatch after round-trip");
        }
    }

    #[tokio::test]
    async fn test_seq_ordering() {
        let (client, _tmp) = make_client().await;
        let session_id = "sess-seq";
        let project_key = 2i64;

        let ts = test_ts();
        persist_event(
            &client,
            &ProviderEvent::Text {
                text: "first".into(),
                timestamp: ts,
            },
            session_id,
            project_key,
        )
        .await
        .unwrap();
        persist_event(
            &client,
            &ProviderEvent::Text {
                text: "second".into(),
                timestamp: ts,
            },
            session_id,
            project_key,
        )
        .await
        .unwrap();
        persist_event(
            &client,
            &ProviderEvent::Text {
                text: "third".into(),
                timestamp: ts,
            },
            session_id,
            project_key,
        )
        .await
        .unwrap();

        let messages = client
            .query_map::<Message, _>(
                "SELECT project_key, session_id, seq, entry_json FROM messages WHERE project_key = $1 AND session_id = $2 ORDER BY seq ASC",
                hiqlite::params!(project_key, session_id),
            )
            .await
            .unwrap();

        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].seq, 1);
        assert_eq!(messages[1].seq, 2);
        assert_eq!(messages[2].seq, 3);
    }

    #[tokio::test]
    async fn test_persist_multiple_sessions() {
        let (client, _tmp) = make_client().await;
        let project_key = 3i64;

        let ts = test_ts();
        persist_event(
            &client,
            &ProviderEvent::Text {
                text: "sess1-event".into(),
                timestamp: ts,
            },
            "sess-a",
            project_key,
        )
        .await
        .unwrap();
        persist_event(
            &client,
            &ProviderEvent::Text {
                text: "sess2-event".into(),
                timestamp: ts,
            },
            "sess-b",
            project_key,
        )
        .await
        .unwrap();

        let loaded_a = load_transcript(&client, "sess-a", project_key)
            .await
            .unwrap();
        let loaded_b = load_transcript(&client, "sess-b", project_key)
            .await
            .unwrap();

        assert_eq!(loaded_a.len(), 1);
        assert_eq!(loaded_b.len(), 1);
        let ts_str = test_ts().format("%Y-%m-%dT%H:%M:%S").to_string();
        assert_eq!(
            serde_json::to_value(&loaded_a[0]).unwrap(),
            serde_json::json!({"type": "text", "text": "sess1-event", "timestamp": ts_str})
        );
        assert_eq!(
            serde_json::to_value(&loaded_b[0]).unwrap(),
            serde_json::json!({"type": "text", "text": "sess2-event", "timestamp": ts_str})
        );
    }

    #[tokio::test]
    async fn test_load_empty_transcript() {
        let (client, _tmp) = make_client().await;
        let loaded = load_transcript(&client, "nonexistent", 9999).await.unwrap();
        assert!(loaded.is_empty());
    }

    #[tokio::test]
    async fn test_persist_event_concurrent() {
        let (client, _tmp) = make_client().await;
        let session_id = "sess-concurrent";
        let project_key = 42i64;
        let ts = test_ts();
        let count = 20;

        let mut handles = Vec::with_capacity(count);
        for i in 0..count {
            let event = ProviderEvent::Text {
                text: format!("msg-{i}"),
                timestamp: ts,
            };
            let c = client.clone();
            handles.push(tokio::spawn(async move {
                persist_event(&c, &event, session_id, project_key).await
            }));
        }

        for h in handles {
            h.await.unwrap().unwrap();
        }

        let messages = client
            .query_map::<Message, _>(
                "SELECT project_key, session_id, seq, entry_json FROM messages WHERE project_key = $1 AND session_id = $2 ORDER BY seq ASC",
                hiqlite::params!(project_key, session_id),
            )
            .await
            .unwrap();

        assert_eq!(messages.len(), count, "all {count} events persisted");

        let mut seen = std::collections::BTreeSet::new();
        for m in &messages {
            assert!(seen.insert(m.seq), "seq {} is a duplicate", m.seq);
        }

        for (i, m) in messages.iter().enumerate() {
            assert_eq!(m.seq, (i + 1) as i32, "seq values must be 1..{count}");
        }
    }

    #[tokio::test]
    async fn test_update_tool_event_updates_row() {
        let (client, _tmp) = make_client().await;
        let session_id = "sess-update";
        let project_key = 10i64;
        let ts = test_ts();

        let original = ProviderEvent::ToolUse {
            tool_name: "read".into(),
            tool_use_id: Some("call1".into()),
            input: serde_json::json!({"path": "/tmp"}),
            result: None,
            message_id: None,
            timestamp: ts,
        };

        persist_event(&client, &original, session_id, project_key)
            .await
            .unwrap();

        let merged = ProviderEvent::ToolUse {
            tool_name: "read".into(),
            tool_use_id: Some("call1".into()),
            input: serde_json::json!({"path": "/tmp"}),
            result: Some("file content".into()),
            message_id: None,
            timestamp: ts,
        };

        let updated = update_tool_event(&client, "call1", &merged, session_id, project_key)
            .await
            .unwrap();
        assert!(updated, "should have found and updated the row");

        let loaded = load_transcript(&client, session_id, project_key)
            .await
            .unwrap();
        assert_eq!(loaded.len(), 1);
        match &loaded[0] {
            ProviderEvent::ToolUse { result, .. } => {
                assert_eq!(result, &Some("file content".into()));
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[tokio::test]
    async fn test_update_tool_event_nonexistent_id() {
        let (client, _tmp) = make_client().await;
        let session_id = "sess-noexist";
        let project_key = 11i64;
        let ts = test_ts();

        let original = ProviderEvent::ToolUse {
            tool_name: "read".into(),
            tool_use_id: Some("call1".into()),
            input: serde_json::json!({"path": "/tmp"}),
            result: None,
            message_id: None,
            timestamp: ts,
        };

        persist_event(&client, &original, session_id, project_key)
            .await
            .unwrap();

        let merged = ProviderEvent::ToolUse {
            tool_name: "read".into(),
            tool_use_id: Some("call_notfound".into()),
            input: serde_json::json!({"path": "/tmp"}),
            result: Some("output".into()),
            message_id: None,
            timestamp: ts,
        };

        // Should return Ok(false) without modifying anything
        let updated = update_tool_event(&client, "call_notfound", &merged, session_id, project_key)
            .await
            .unwrap();
        assert!(!updated, "should not have found a matching row");

        let loaded = load_transcript(&client, session_id, project_key)
            .await
            .unwrap();
        assert_eq!(loaded.len(), 1);
        match &loaded[0] {
            ProviderEvent::ToolUse { result, .. } => {
                assert!(result.is_none(), "should not have been updated");
            }
            _ => panic!("expected ToolUse"),
        }
    }
}
