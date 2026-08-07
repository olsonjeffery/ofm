use axum::extract::{Path, State};
use axum::middleware;
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::{AuthRejection, AuthUser};
use crate::db::schema::GroupLevel;
use crate::server::error::ServerError;
use crate::server::state::AppState;
use crate::services::groups::{self, GroupError};

impl From<GroupError> for ServerError {
    fn from(e: GroupError) -> Self {
        match e {
            GroupError::NotFound => ServerError::NotFound("Group not found".into()),
            GroupError::Forbidden => ServerError::Forbidden("forbidden".into()),
            GroupError::BadRequest(m) => ServerError::BadRequest(m),
            GroupError::Db(m) => ServerError::Internal(m),
        }
    }
}

async fn require_admin(
    auth: AuthUser,
    request: axum::extract::Request,
    next: middleware::Next,
) -> Result<axum::response::Response, AuthRejection> {
    if !auth.is_admin {
        return Err(AuthRejection::Forbidden);
    }
    Ok(next.run(request).await)
}

pub fn groups_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_groups).post(create_group))
        .route("/scopes-available", get(scopes_available))
        .route(
            "/{id}",
            get(get_group).put(update_group).delete(delete_group),
        )
        .route("/{id}/members", get(list_members).post(add_member))
        .route(
            "/{id}/members/{member_id}",
            put(change_member_level).delete(remove_member),
        )
        .layer(middleware::from_fn(require_admin))
}

#[derive(Deserialize)]
struct CreateGroupRequest {
    name: String,
    #[serde(default)]
    is_org: bool,
    #[serde(default)]
    is_oauth_scope: bool,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    owner_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct UpdateGroupRequest {
    name: Option<String>,
    title: Option<String>,
    description: Option<String>,
    owner_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct AddMemberRequest {
    user_id: Option<Uuid>,
    member_group_id: Option<Uuid>,
    #[serde(default)]
    level: Option<String>,
}

#[derive(Deserialize)]
struct ChangeLevelRequest {
    level: String,
}

fn parse_level(level: &str) -> Result<GroupLevel, ServerError> {
    level.parse().map_err(ServerError::BadRequest)
}

async fn username_for_user(state: &AppState, user_id: Uuid) -> Result<String, ServerError> {
    let mut rows = state
        .db
        .query_raw(
            "SELECT username FROM users WHERE id = $1",
            hiqlite::params!(user_id.to_string()),
        )
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(rows
        .first_mut()
        .map(|r| r.get::<String>("username"))
        .unwrap_or_default())
}

/// Enrich a group row for the admin UI: owner username, effective member
/// count, and whether it is the bootstrap `admins` group.
async fn group_payload(
    state: &AppState,
    group: &crate::db::schema::Group,
) -> Result<serde_json::Value, ServerError> {
    let owner_username = username_for_user(state, group.owner_id).await?;

    let member_count = groups::resolve_members(&state.db, &group.id)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(json!({
        "id": group.id,
        "name": group.name,
        "is_org": group.is_org,
        "is_oauth_scope": group.is_oauth_scope,
        "title": group.title,
        "description": group.description,
        "owner_id": group.owner_id,
        "owner_username": owner_username,
        "created_by": group.created_by,
        "created_at": group.created_at,
        "member_count": member_count,
        "is_admins_group": group.name == "admins",
    }))
}

async fn list_groups(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, ServerError> {
    let groups = groups::list_groups(&state.db).await?;
    let mut payload = Vec::with_capacity(groups.len());
    for group in groups {
        payload.push(group_payload(&state, &group).await?);
    }
    Ok(Json(json!({ "groups": payload })))
}

async fn create_group(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateGroupRequest>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ServerError> {
    let group = groups::create_group(
        &state.db,
        &auth,
        &body.name,
        body.is_org,
        body.is_oauth_scope,
        &body.title,
        &body.description,
        body.owner_id,
    )
    .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(group_payload(&state, &group).await?),
    ))
}

async fn get_group(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ServerError> {
    let group = groups::get_group(&state.db, id).await?;
    Ok(Json(group_payload(&state, &group).await?))
}

async fn update_group(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateGroupRequest>,
) -> Result<Json<serde_json::Value>, ServerError> {
    let group = groups::update_group(
        &state.db,
        &auth,
        id,
        body.name.as_deref(),
        body.title.as_deref(),
        body.description.as_deref(),
        body.owner_id,
    )
    .await?;
    Ok(Json(group_payload(&state, &group).await?))
}

async fn delete_group(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ServerError> {
    groups::delete_group(&state.db, &auth, id).await?;
    Ok(Json(json!({ "success": true })))
}

async fn scopes_available(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, ServerError> {
    let mut scopes: Vec<String> = state
        .oidc_provider
        .as_ref()
        .map(|o| o.scopes_supported.clone())
        .unwrap_or_default();
    // Union in admin-entered scope names (groups created with is_oauth_scope).
    scopes.extend(
        groups::list_groups(&state.db)
            .await?
            .into_iter()
            .filter(|g| g.is_oauth_scope)
            .map(|g| g.name),
    );
    scopes.sort();
    scopes.dedup();
    Ok(Json(json!({ "scopes": scopes })))
}

async fn list_members(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ServerError> {
    groups::get_group(&state.db, id).await?;
    let members = groups::list_members(&state.db, id).await?;

    let mut member_payload = Vec::with_capacity(members.len());
    for member in members {
        let (username, subgroup_name) = if let Some(uid) = member.user_id {
            (Some(username_for_user(&state, uid).await?), None)
        } else if let Some(sgid) = member.member_group_id {
            let name = groups::get_group(&state.db, sgid)
                .await
                .map(|g| g.name)
                .unwrap_or_default();
            (None, Some(name))
        } else {
            (None, None)
        };

        member_payload.push(json!({
            "id": member.id,
            "group_id": member.group_id,
            "user_id": member.user_id,
            "username": username,
            "member_group_id": member.member_group_id,
            "subgroup_name": subgroup_name,
            "level": member.level,
            "created_at": member.created_at,
        }));
    }

    Ok(Json(json!({ "members": member_payload })))
}

async fn add_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<AddMemberRequest>,
) -> Result<(axum::http::StatusCode, Json<serde_json::Value>), ServerError> {
    let level = parse_level(body.level.as_deref().unwrap_or("read-only"))?;
    let member = groups::add_member(
        &state.db,
        &auth,
        id,
        body.user_id,
        body.member_group_id,
        level,
    )
    .await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(json!({ "id": member.id })),
    ))
}

async fn change_member_level(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((group_id, member_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ChangeLevelRequest>,
) -> Result<Json<serde_json::Value>, ServerError> {
    let level = parse_level(&body.level)?;
    let member = groups::change_level(&state.db, &auth, group_id, member_id, level).await?;
    Ok(Json(json!({ "id": member.id, "level": member.level })))
}

async fn remove_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((group_id, member_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, ServerError> {
    groups::remove_member(&state.db, &auth, group_id, member_id).await?;
    Ok(Json(json!({ "success": true })))
}
