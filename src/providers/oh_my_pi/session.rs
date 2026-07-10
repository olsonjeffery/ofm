use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use super::OhMyPiSession;

pub async fn register_session(
    sessions: &Arc<Mutex<HashMap<String, OhMyPiSession>>>,
    session_id: &str,
    session: OhMyPiSession,
) {
    sessions
        .lock()
        .await
        .insert(session_id.to_string(), session);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_session() {
        let sessions: Arc<Mutex<HashMap<String, OhMyPiSession>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let system = portable_pty::native_pty_system();
        let pair = system.openpty(portable_pty::PtySize::default()).unwrap();
        let cmd = portable_pty::CommandBuilder::new("true");
        let child = pair.slave.spawn_command(cmd).unwrap();
        let pid = child.process_id().unwrap_or(0);
        let mock_session = OhMyPiSession { pid, child, pair };

        register_session(&sessions, "sess-1", mock_session).await;

        let map = sessions.lock().await;
        assert!(map.contains_key("sess-1"));
    }
}
