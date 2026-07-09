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

pub async fn validate_session(session: &Session, db: &hiqlite::Client) -> bool {
    let mut rows = db
        .query_raw(
            "SELECT token_version FROM users WHERE id = $1",
            hiqlite::params!(session.user.id.to_string()),
        )
        .await;

    match &mut rows {
        Ok(rows) => {
            let Some(row) = rows.first_mut() else {
                return false;
            };
            let current_version: i64 = row.get("token_version");
            session.user.token_version >= current_version as i32
        }
        Err(_) => false,
    }
}
