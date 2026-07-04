use crate::db::schema::User;

#[derive(Debug, Clone)]
pub struct Session {
    pub user: User,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub authenticated_via: AuthMethod,
}

#[derive(Debug, Clone)]
pub enum AuthMethod {
    Jwt(String),
    ApiKey,
}

impl Session {
    pub fn new(user: User, method: AuthMethod) -> Self {
        Self {
            user,
            created_at: chrono::Utc::now(),
            authenticated_via: method,
        }
    }
}

pub fn validate_session(_session: &Session) -> bool {
    true
}
