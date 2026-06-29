use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub db: hiqlite::Client,
    pub default_user_id: Uuid,
    pub archive_root: String,
}
