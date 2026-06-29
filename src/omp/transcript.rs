use hiqlite::Client;

use crate::db::schema::Message;
use crate::omp::OmpRpcEvent;

pub async fn persist_event(
    client: &Client,
    event: &OmpRpcEvent,
    session_id: &str,
    project_key: &str,
) -> Result<(), hiqlite::Error> {
    let seq: i32 = next_seq(client, session_id, project_key).await?;
    let entry_json = serde_json::to_value(event)
        .map_err(|e| hiqlite::Error::new(format!("serialize event: {e}")))?;

    client
        .execute(
            "INSERT INTO messages (project_key, session_id, seq, entry_json) VALUES ($1, $2, $3, $4)",
            hiqlite::params!(project_key, session_id, seq, entry_json.to_string()),
        )
        .await?;
    Ok(())
}

async fn next_seq(
    client: &Client,
    session_id: &str,
    project_key: &str,
) -> Result<i32, hiqlite::Error> {
    let mut rows = client
        .query_raw(
            "SELECT COALESCE(MAX(seq), 0) + 1 AS next_seq FROM messages WHERE project_key = $1 AND session_id = $2",
            hiqlite::params!(project_key, session_id),
        )
        .await?;
    let seq: i64 = rows.first_mut().map(|r| r.get("next_seq")).unwrap_or(1);
    Ok(seq as i32)
}

pub async fn load_transcript(
    client: &Client,
    session_id: &str,
    project_key: &str,
) -> Result<Vec<OmpRpcEvent>, hiqlite::Error> {
    let messages = client
        .query_map::<Message, _>(
            "SELECT project_key, session_id, seq, entry_json FROM messages WHERE project_key = $1 AND session_id = $2 ORDER BY seq ASC",
            hiqlite::params!(project_key, session_id),
        )
        .await?;

    messages
        .into_iter()
        .map(|m| {
            serde_json::from_value(m.entry_json)
                .map_err(|e| hiqlite::Error::new(format!("deserialize event: {e}")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use tempfile::TempDir;

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

    fn make_events() -> Vec<OmpRpcEvent> {
        vec![
            OmpRpcEvent::Text {
                text: "hello".into(),
            },
            OmpRpcEvent::ToolUse {
                tool_name: "read".into(),
                tool_use_id: Some("id1".into()),
                input: serde_json::json!({"path": "/tmp"}),
            },
            OmpRpcEvent::ToolResult {
                tool_use_id: Some("id1".into()),
                result: "ok".into(),
            },
            OmpRpcEvent::Thinking {
                thinking: "hmm".into(),
            },
            OmpRpcEvent::Done(serde_json::json!({"status": "ok"})),
        ]
    }

    #[tokio::test]
    async fn test_persist_and_load_transcript() {
        let (client, _tmp) = make_client().await;
        let session_id = "sess-1";
        let project_key = "proj-1";
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
        let project_key = "proj-seq";

        persist_event(&client, &OmpRpcEvent::Text { text: "first".into() }, session_id, project_key).await.unwrap();
        persist_event(&client, &OmpRpcEvent::Text { text: "second".into() }, session_id, project_key).await.unwrap();
        persist_event(&client, &OmpRpcEvent::Text { text: "third".into() }, session_id, project_key).await.unwrap();

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
        let project_key = "proj-multi";

        persist_event(&client, &OmpRpcEvent::Text { text: "sess1-event".into() }, "sess-a", project_key).await.unwrap();
        persist_event(&client, &OmpRpcEvent::Text { text: "sess2-event".into() }, "sess-b", project_key).await.unwrap();

        let loaded_a = load_transcript(&client, "sess-a", project_key).await.unwrap();
        let loaded_b = load_transcript(&client, "sess-b", project_key).await.unwrap();

        assert_eq!(loaded_a.len(), 1);
        assert_eq!(loaded_b.len(), 1);
        assert_eq!(
            serde_json::to_value(&loaded_a[0]).unwrap(),
            serde_json::json!({"type": "text", "text": "sess1-event"})
        );
        assert_eq!(
            serde_json::to_value(&loaded_b[0]).unwrap(),
            serde_json::json!({"type": "text", "text": "sess2-event"})
        );
    }

    #[tokio::test]
    async fn test_load_empty_transcript() {
        let (client, _tmp) = make_client().await;
        let loaded = load_transcript(&client, "nonexistent", "noproj").await.unwrap();
        assert!(loaded.is_empty());
    }
}
