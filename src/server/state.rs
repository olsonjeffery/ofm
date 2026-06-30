use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::omp::OmpSession;

#[derive(Clone)]
pub struct AppState {
    pub db: hiqlite::Client,
    pub default_user_id: Uuid,
    pub archive_root: String,
    pub omp_sessions: Arc<Mutex<HashMap<String, OmpSession>>>,
}
