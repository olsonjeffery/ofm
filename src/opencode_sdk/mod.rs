pub mod client;
pub mod conversation;
pub mod server;
pub mod types;

pub use client::{create_opencode_client, EventStream, OpencodeClient};
pub use conversation::{
    one_shot, OneShotConfig, PhaseConfig, PhaseConversation, PhaseEventStream,
    UnstructuredConversation,
};
pub use server::{create_opencode_server, OpenCodeServer, ServerOptions};
pub use types::*;

// ── SdkError ──────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Timeout")]
    Timeout,
}

// ── Convenience factory ───────────────────────────────────────────────────

pub async fn create_opencode(
    options: ServerOptions,
) -> Result<(OpencodeClient, OpenCodeServer), SdkError> {
    let server = create_opencode_server(options).await?;
    let password = server.password().map(|s| s.to_string());
    let client = OpencodeClient::new(&server.url(), password.as_deref());
    Ok((client, server))
}
