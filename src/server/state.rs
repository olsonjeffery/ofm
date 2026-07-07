use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use axum::extract::FromRef;
use cookie::Key;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::auth::jwks::JwksCache;
use crate::omp::OmpSession;
use crate::providers::LlmProvider;

type SharedJwksCache = Option<Arc<RwLock<Option<JwksCache>>>>;

#[derive(Clone)]
pub struct AppState {
    pub db: hiqlite::Client,
    pub default_user_id: Uuid,
    pub archive_root: String,
    pub config_root: String,
    pub omp_sessions: Arc<Mutex<HashMap<String, OmpSession>>>,
    pub active_sessions: Arc<Mutex<HashMap<String, Box<dyn LlmProvider>>>>,
    pub oidc_provider: Option<OidcEndpoints>,
    pub pkce_store: Arc<Mutex<HashMap<String, PkceEntry>>>,
    pub cookie_key: Key,
    pub api_key_pepper: Vec<u8>,
    pub rauthy_base_url: Option<String>,
}

impl FromRef<AppState> for Key {
    fn from_ref(state: &AppState) -> Self {
        state.cookie_key.clone()
    }
}

#[derive(Debug, Clone)]
pub struct OidcEndpoints {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub revocation_endpoint: Option<String>,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
    pub jwks_cache: SharedJwksCache,
    pub jwks_issuer: Option<String>,
}

pub struct PkceEntry {
    pub code_verifier: String,
    pub csrf_state: String,
    pub created_at: Instant,
}
