use std::collections::{HashMap, HashSet};

use hiqlite::Client;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::schema::{Group, GroupLevel, GroupMember};

#[derive(Debug, thiserror::Error)]
pub enum GroupError {
    #[error("group not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
    #[error("{0}")]
    BadRequest(String),
    #[error("database error: {0}")]
    Db(String),
}

fn utc_now() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn db_err(e: hiqlite::Error) -> GroupError {
    GroupError::Db(e.to_string())
}

fn is_no_rows(e: &hiqlite::Error) -> bool {
    e.to_string().contains("no rows returned")
}

/// Map a single-row lookup error: an empty result set is `NotFound`, anything
/// else is a generic database error.
fn map_group_error(e: hiqlite::Error) -> GroupError {
    if is_no_rows(&e) {
        GroupError::NotFound
    } else {
        db_err(e)
    }
}

fn insert_level(map: &mut HashMap<Uuid, GroupLevel>, user_id: Uuid, level: GroupLevel) {
    map.entry(user_id)
        .and_modify(|existing| *existing = (*existing).max(level))
        .or_insert(level);
}

fn is_owner_or_admin(group: &Group, actor: &AuthUser) -> bool {
    actor.is_admin || group.owner_id == actor.user_id
}

/// Map an INSERT/UPDATE failure onto `BadRequest` when it is a unique-constraint
/// violation on `groups.name`, else the generic database error.
fn map_unique_error(e: hiqlite::Error, name: &str) -> GroupError {
    if e.to_string().contains("UNIQUE") {
        GroupError::BadRequest(format!("a group named '{name}' already exists"))
    } else {
        db_err(e)
    }
}

// ── Group definition CRUD ────────────────────────────────────────────────────

/// Create a group. Admin-only. The creating admin becomes an `admin`-level
/// member and (by default) the group owner.
#[allow(clippy::too_many_arguments)]
pub async fn create_group(
    client: &Client,
    actor: &AuthUser,
    name: &str,
    is_org: bool,
    is_oauth_scope: bool,
    title: &str,
    description: &str,
    owner_id: Option<Uuid>,
) -> Result<Group, GroupError> {
    if !actor.is_admin {
        return Err(GroupError::Forbidden);
    }
    let name = name.trim();
    if name.is_empty() {
        return Err(GroupError::BadRequest("name is required".into()));
    }
    let title = title.trim();
    let description = description.trim();
    let owner_id = owner_id.unwrap_or(actor.user_id);

    let id = Uuid::new_v4();
    let now = utc_now();
    let insert = client
        .execute(
            "INSERT INTO groups (id, name, is_org, is_oauth_scope, title, description, owner_id, created_by, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            hiqlite::params!(
                id.to_string(),
                name,
                is_org as i64,
                is_oauth_scope as i64,
                title,
                description,
                owner_id.to_string(),
                actor.username.clone(),
                now
            ),
        )
        .await;
    if let Err(e) = insert {
        return Err(map_unique_error(e, name));
    }

    // The creating admin is always an admin-level member.
    insert_membership_row(client, &id, Some(actor.user_id), None, GroupLevel::Admin).await?;

    get_group(client, id).await
}

pub async fn get_group(client: &Client, group_id: Uuid) -> Result<Group, GroupError> {
    client
        .query_map_one::<Group, _>(
            "SELECT id, name, is_org, is_oauth_scope, title, description, owner_id, created_by, created_at \
             FROM groups WHERE id = $1",
            hiqlite::params!(group_id.to_string()),
        )
        .await
        .map_err(map_group_error)
}

pub async fn get_group_by_name(client: &Client, name: &str) -> Result<Group, GroupError> {
    client
        .query_map_one::<Group, _>(
            "SELECT id, name, is_org, is_oauth_scope, title, description, owner_id, created_by, created_at \
             FROM groups WHERE name = $1",
            hiqlite::params!(name),
        )
        .await
        .map_err(map_group_error)
}

pub async fn list_groups(client: &Client) -> Result<Vec<Group>, GroupError> {
    client
        .query_map::<Group, _>(
            "SELECT id, name, is_org, is_oauth_scope, title, description, owner_id, created_by, created_at \
             FROM groups ORDER BY name",
            hiqlite::params!(),
        )
        .await
        .map_err(db_err)
}

/// Update a group's name/title/description (owner or admin) and/or owner
/// (admin-only).
pub async fn update_group(
    client: &Client,
    actor: &AuthUser,
    group_id: Uuid,
    name: Option<&str>,
    title: Option<&str>,
    description: Option<&str>,
    owner_id: Option<Uuid>,
) -> Result<Group, GroupError> {
    let group = get_group(client, group_id).await?;

    // Renaming/title/description edits are owner-or-admin; changing the owner
    // is admin-only.
    let owner_change = owner_id.is_some() && owner_id != Some(group.owner_id);
    let authorized = if owner_change {
        actor.is_admin
    } else {
        is_owner_or_admin(&group, actor)
    };
    if !authorized {
        return Err(GroupError::Forbidden);
    }

    let name = name.map(str::trim);
    if name.is_some_and(|n| n.is_empty()) {
        return Err(GroupError::BadRequest("name must not be empty".into()));
    }
    let title = title.map(str::trim);
    let description = description.map(str::trim);

    let owner_param = if owner_change {
        owner_id
    } else {
        Some(group.owner_id)
    };

    let update = client
        .execute(
            "UPDATE groups SET name = COALESCE($1, name), title = COALESCE($2, title), \
             description = COALESCE($3, description), owner_id = $4 WHERE id = $5",
            hiqlite::params!(
                name,
                title,
                description,
                owner_param.map(|id| id.to_string()),
                group_id.to_string()
            ),
        )
        .await;
    if let Err(e) = update {
        return Err(map_unique_error(e, name.unwrap_or(&group.name)));
    }

    get_group(client, group_id).await
}

/// Delete a group. Admin or group owner.
pub async fn delete_group(
    client: &Client,
    actor: &AuthUser,
    group_id: Uuid,
) -> Result<(), GroupError> {
    let group = get_group(client, group_id).await?;
    if !is_owner_or_admin(&group, actor) {
        return Err(GroupError::Forbidden);
    }
    client
        .execute(
            "DELETE FROM groups WHERE id = $1",
            hiqlite::params!(group_id.to_string()),
        )
        .await
        .map_err(db_err)?;
    Ok(())
}

// ── Membership ───────────────────────────────────────────────────────────────

async fn insert_membership_row(
    client: &Client,
    group_id: &Uuid,
    user_id: Option<Uuid>,
    member_group_id: Option<Uuid>,
    level: GroupLevel,
) -> Result<GroupMember, GroupError> {
    let id = Uuid::new_v4();
    let now = utc_now();
    let insert = client
        .execute(
            "INSERT INTO group_members (id, group_id, user_id, member_group_id, level, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6)",
            hiqlite::params!(
                id.to_string(),
                group_id.to_string(),
                user_id.map(|u| u.to_string()),
                member_group_id.map(|u| u.to_string()),
                level.to_string(),
                now
            ),
        )
        .await;
    if let Err(e) = insert {
        if e.to_string().contains("UNIQUE") {
            return Err(GroupError::BadRequest(
                "member is already enrolled in this group".into(),
            ));
        }
        return Err(db_err(e));
    }
    client
        .query_map_one::<GroupMember, _>(
            "SELECT id, group_id, user_id, member_group_id, level, created_at \
             FROM group_members WHERE id = $1",
            hiqlite::params!(id.to_string()),
        )
        .await
        .map_err(db_err)
}

/// Add a user or subgroup as a member. Owner or admin. Exactly one of
/// `user_id` / `member_group_id` must be provided.
pub async fn add_member(
    client: &Client,
    actor: &AuthUser,
    group_id: Uuid,
    user_id: Option<Uuid>,
    member_group_id: Option<Uuid>,
    level: GroupLevel,
) -> Result<GroupMember, GroupError> {
    let group = get_group(client, group_id).await?;
    if !is_owner_or_admin(&group, actor) {
        return Err(GroupError::Forbidden);
    }

    match (user_id, member_group_id) {
        (Some(_), Some(_)) => {
            return Err(GroupError::BadRequest(
                "a membership row must target exactly one of user_id or member_group_id".into(),
            ));
        }
        (None, None) => {
            return Err(GroupError::BadRequest(
                "a membership row must target a user or a subgroup".into(),
            ));
        }
        (Some(uid), None) => {
            let rows = client
                .query_raw(
                    "SELECT id FROM users WHERE id = $1",
                    hiqlite::params!(uid.to_string()),
                )
                .await
                .map_err(db_err)?;
            if rows.is_empty() {
                return Err(GroupError::BadRequest("user not found".into()));
            }
        }
        (None, Some(sgid)) => {
            if sgid == group_id {
                return Err(GroupError::BadRequest(
                    "a group cannot be a member of itself".into(),
                ));
            }
            if get_group(client, sgid).await.is_err() {
                return Err(GroupError::BadRequest("subgroup not found".into()));
            }
            // Prevent groups-of-groups cycles: adding S as a member of G is
            // illegal if G is reachable from S through subgroup edges.
            if subgroup_reachable(client, sgid, group_id).await? {
                return Err(GroupError::BadRequest(
                    "adding this subgroup would create a membership cycle".into(),
                ));
            }
        }
    }

    insert_membership_row(client, &group_id, user_id, member_group_id, level).await
}

pub async fn remove_member(
    client: &Client,
    actor: &AuthUser,
    group_id: Uuid,
    member_id: Uuid,
) -> Result<(), GroupError> {
    let group = get_group(client, group_id).await?;
    if !is_owner_or_admin(&group, actor) {
        return Err(GroupError::Forbidden);
    }
    let rows = client
        .execute(
            "DELETE FROM group_members WHERE id = $1 AND group_id = $2",
            hiqlite::params!(member_id.to_string(), group_id.to_string()),
        )
        .await
        .map_err(db_err)?;
    if rows == 0 {
        return Err(GroupError::NotFound);
    }
    Ok(())
}

pub async fn change_level(
    client: &Client,
    actor: &AuthUser,
    group_id: Uuid,
    member_id: Uuid,
    level: GroupLevel,
) -> Result<GroupMember, GroupError> {
    let group = get_group(client, group_id).await?;
    if !is_owner_or_admin(&group, actor) {
        return Err(GroupError::Forbidden);
    }
    let rows = client
        .execute(
            "UPDATE group_members SET level = $1 WHERE id = $2 AND group_id = $3",
            hiqlite::params!(
                level.to_string(),
                member_id.to_string(),
                group_id.to_string()
            ),
        )
        .await
        .map_err(db_err)?;
    if rows == 0 {
        return Err(GroupError::NotFound);
    }
    let member = client
        .query_map_one::<GroupMember, _>(
            "SELECT id, group_id, user_id, member_group_id, level, created_at \
             FROM group_members WHERE id = $1",
            hiqlite::params!(member_id.to_string()),
        )
        .await
        .map_err(db_err)?;
    Ok(member)
}

pub async fn list_members(client: &Client, group_id: Uuid) -> Result<Vec<GroupMember>, GroupError> {
    client
        .query_map::<GroupMember, _>(
            "SELECT id, group_id, user_id, member_group_id, level, created_at \
             FROM group_members WHERE group_id = $1 ORDER BY created_at",
            hiqlite::params!(group_id.to_string()),
        )
        .await
        .map_err(db_err)
}

// ── Membership resolution ────────────────────────────────────────────────────

/// Fetch the `(target, level)` pairs from `group_members` for one target kind.
/// `target` is a compile-time constant (`"user_id"` or `"member_group_id"`) that
/// selects both the column to read and a static SQL statement.
async fn member_rows(
    client: &Client,
    group_id: &Uuid,
    target: &'static str,
) -> Result<Vec<(Uuid, GroupLevel)>, GroupError> {
    let sql = if target == "user_id" {
        "SELECT user_id, level FROM group_members WHERE group_id = $1 AND user_id IS NOT NULL"
    } else {
        "SELECT member_group_id, level FROM group_members WHERE group_id = $1 AND member_group_id IS NOT NULL"
    };
    let mut rows = client
        .query_raw(sql, hiqlite::params!(group_id.to_string()))
        .await
        .map_err(db_err)?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows.iter_mut() {
        let id: String = row.get(target);
        let level: String = row.get("level");
        let level: GroupLevel = level.parse().map_err(GroupError::BadRequest)?;
        let id = Uuid::parse_str(&id).map_err(|e| {
            GroupError::BadRequest(format!("invalid {target} in group_members: {e}"))
        })?;
        out.push((id, level));
    }
    Ok(out)
}

async fn direct_user_members(
    client: &Client,
    group_id: &Uuid,
) -> Result<Vec<(Uuid, GroupLevel)>, GroupError> {
    member_rows(client, group_id, "user_id").await
}

async fn subgroup_rows(
    client: &Client,
    group_id: &Uuid,
) -> Result<Vec<(Uuid, GroupLevel)>, GroupError> {
    member_rows(client, group_id, "member_group_id").await
}

async fn users_with_scope(client: &Client, scope_name: &str) -> Result<Vec<Uuid>, GroupError> {
    let mut rows = client
        .query_raw(
            "SELECT id, scopes FROM users WHERE scopes != ''",
            hiqlite::params!(),
        )
        .await
        .map_err(db_err)?;
    let mut out = Vec::new();
    for row in rows.iter_mut() {
        let id: String = row.get("id");
        let scopes: String = row.get("scopes");
        if scopes.split_whitespace().any(|s| s == scope_name) {
            if let Ok(uid) = Uuid::parse_str(&id) {
                out.push(uid);
            }
        }
    }
    Ok(out)
}

/// Collect every user's effective level in `group_id`, merging the highest
/// level across all paths (direct membership, subgroup roll-ups capped by the
/// subgroup's membership level, scope-derived membership) plus the implicit
/// admin level for the group owner. Cycle-safe.
///
/// Implemented with an explicit work stack rather than async recursion (which
/// would need a boxed future): each frame tracks the path's `visited` set
/// (cycle guard) and the level cap imposed by the subgroup edge that led here.
async fn effective_levels(
    client: &Client,
    group_id: &Uuid,
) -> Result<HashMap<Uuid, GroupLevel>, GroupError> {
    let mut map = HashMap::new();
    let mut stack = vec![(*group_id, GroupLevel::Admin, HashSet::new())];
    while let Some((gid, cap, mut visited)) = stack.pop() {
        if !visited.insert(gid) {
            continue;
        }
        let group = get_group(client, gid).await?;
        insert_level(&mut map, group.owner_id, GroupLevel::Admin.min(cap));
        for (uid, level) in direct_user_members(client, &gid).await? {
            insert_level(&mut map, uid, level.min(cap));
        }
        if group.is_oauth_scope {
            for uid in users_with_scope(client, &group.name).await? {
                insert_level(&mut map, uid, GroupLevel::ReadOnly.min(cap));
            }
        }
        for (sgid, subgroup_level) in subgroup_rows(client, &gid).await? {
            stack.push((sgid, subgroup_level.min(cap), visited.clone()));
        }
    }
    Ok(map)
}

/// Effective membership level of `user_id` in `group_id` (None if not a member).
pub async fn is_member(
    client: &Client,
    group_id: &Uuid,
    user_id: Uuid,
) -> Result<Option<GroupLevel>, GroupError> {
    Ok(effective_levels(client, group_id)
        .await?
        .get(&user_id)
        .copied())
}

/// The complete set of user ids that are members of `group_id` through any path.
pub async fn resolve_members(
    client: &Client,
    group_id: &Uuid,
) -> Result<HashSet<Uuid>, GroupError> {
    Ok(effective_levels(client, group_id)
        .await?
        .into_keys()
        .collect())
}

/// Effective membership level for an authenticated user, with server admins
/// implicitly holding `admin` everywhere.
pub async fn has_group_access(
    client: &Client,
    group_id: &Uuid,
    user: &AuthUser,
) -> Result<Option<GroupLevel>, GroupError> {
    if user.is_admin {
        return Ok(Some(GroupLevel::Admin));
    }
    is_member(client, group_id, user.user_id).await
}

/// All groups where `user_id` holds some effective membership (direct,
/// subgroup roll-up, ownership, or scope-derived).
pub async fn groups_for_user(client: &Client, user_id: Uuid) -> Result<Vec<Uuid>, GroupError> {
    let groups = list_groups(client).await?;
    let mut out = Vec::new();
    for group in groups {
        if is_member(client, &group.id, user_id).await?.is_some() {
            out.push(group.id);
        }
    }
    Ok(out)
}

/// Whether `target` is reachable from `start` by following subgroup edges —
/// used to refuse membership cycles when nesting groups.
async fn subgroup_reachable(
    client: &Client,
    start: Uuid,
    target: Uuid,
) -> Result<bool, GroupError> {
    let mut stack = vec![start];
    let mut visited = HashSet::new();
    while let Some(current) = stack.pop() {
        if current == target {
            return Ok(true);
        }
        if !visited.insert(current) {
            continue;
        }
        for (sgid, _) in subgroup_rows(client, &current).await? {
            stack.push(sgid);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn make_client() -> (Client, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().unwrap();
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
        crate::db::run_migrations(&client).await.unwrap();
        (client, tmp)
    }

    fn auth(user_id: Uuid, is_admin: bool) -> AuthUser {
        AuthUser {
            user_id,
            username: "actor".into(),
            oidc_subject: None,
            is_admin,
            is_technical: false,
        }
    }

    async fn insert_user(client: &Client, id: Uuid, username: &str) {
        let now = utc_now();
        client
            .execute(
                "INSERT INTO users (id, username, is_admin, is_active, created_at) VALUES ($1, $2, 0, 1, $3)",
                hiqlite::params!(id.to_string(), username, now),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_create_group_requires_admin() {
        let (client, _tmp) = make_client().await;
        let owner = Uuid::new_v4();
        insert_user(&client, owner, "owner").await;
        let non_admin = auth(owner, false);
        let err = create_group(&client, &non_admin, "team", false, false, "", "", None)
            .await
            .unwrap_err();
        assert!(matches!(err, GroupError::Forbidden));
    }

    #[tokio::test]
    async fn test_create_group_creator_is_admin_member() {
        let (client, _tmp) = make_client().await;
        let admin_id = Uuid::new_v4();
        insert_user(&client, admin_id, "admin").await;
        let admin = auth(admin_id, true);

        let group = create_group(&client, &admin, "team-a", false, false, "Team A", "", None)
            .await
            .unwrap();
        assert_eq!(group.name, "team-a");
        assert_eq!(group.title, "Team A");
        assert_eq!(group.owner_id, admin_id);
        assert_eq!(group.created_by, "actor");

        assert_eq!(
            is_member(&client, &group.id, admin_id).await.unwrap(),
            Some(GroupLevel::Admin)
        );
    }

    #[tokio::test]
    async fn test_create_group_duplicate_name_rejected() {
        let (client, _tmp) = make_client().await;
        let admin_id = Uuid::new_v4();
        insert_user(&client, admin_id, "admin").await;
        let admin = auth(admin_id, true);
        create_group(&client, &admin, "dup", false, false, "", "", None)
            .await
            .unwrap();
        let err = create_group(&client, &admin, "dup", false, false, "", "", None)
            .await
            .unwrap_err();
        assert!(matches!(err, GroupError::BadRequest(_)));
    }

    #[tokio::test]
    async fn test_list_and_get_group() {
        let (client, _tmp) = make_client().await;
        let admin_id = Uuid::new_v4();
        insert_user(&client, admin_id, "admin").await;
        let admin = auth(admin_id, true);
        let g1 = create_group(&client, &admin, "alpha", false, false, "", "", None)
            .await
            .unwrap();
        let g2 = create_group(&client, &admin, "beta", true, true, "", "", None)
            .await
            .unwrap();

        let groups = list_groups(&client).await.unwrap();
        assert_eq!(groups.len(), 2);

        let fetched = get_group(&client, g1.id).await.unwrap();
        assert_eq!(fetched.name, "alpha");
        assert!(!fetched.is_org);
        assert!(!fetched.is_oauth_scope);

        let fetched = get_group(&client, g2.id).await.unwrap();
        assert!(fetched.is_org);
        assert!(fetched.is_oauth_scope);

        assert!(matches!(
            get_group(&client, Uuid::new_v4()).await.unwrap_err(),
            GroupError::NotFound
        ));
    }

    #[tokio::test]
    async fn test_update_group_permissions() {
        let (client, _tmp) = make_client().await;
        let admin_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let stranger_id = Uuid::new_v4();
        insert_user(&client, admin_id, "admin").await;
        insert_user(&client, owner_id, "owner").await;
        insert_user(&client, stranger_id, "stranger").await;
        let admin = auth(admin_id, true);
        let owner = auth(owner_id, false);
        let stranger = auth(stranger_id, false);

        let group = create_group(
            &client,
            &admin,
            "perm",
            false,
            false,
            "",
            "",
            Some(owner_id),
        )
        .await
        .unwrap();

        // Owner can rename.
        let updated = update_group(&client, &owner, group.id, Some("renamed"), None, None, None)
            .await
            .unwrap();
        assert_eq!(updated.name, "renamed");

        // Stranger cannot rename.
        let err = update_group(
            &client,
            &stranger,
            group.id,
            Some("hacked"),
            None,
            None,
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, GroupError::Forbidden));

        // Only admin can change owner.
        let err = update_group(
            &client,
            &owner,
            group.id,
            None,
            None,
            None,
            Some(stranger_id),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, GroupError::Forbidden));
        let updated = update_group(
            &client,
            &admin,
            group.id,
            None,
            None,
            None,
            Some(stranger_id),
        )
        .await
        .unwrap();
        assert_eq!(updated.owner_id, stranger_id);
    }

    #[tokio::test]
    async fn test_delete_group_owner_or_admin() {
        let (client, _tmp) = make_client().await;
        let admin_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        let stranger_id = Uuid::new_v4();
        insert_user(&client, admin_id, "admin").await;
        insert_user(&client, owner_id, "owner").await;
        insert_user(&client, stranger_id, "stranger").await;
        let admin = auth(admin_id, true);
        let owner = auth(owner_id, false);
        let stranger = auth(stranger_id, false);

        let group = create_group(&client, &admin, "del", false, false, "", "", Some(owner_id))
            .await
            .unwrap();

        let err = delete_group(&client, &stranger, group.id)
            .await
            .unwrap_err();
        assert!(matches!(err, GroupError::Forbidden));

        delete_group(&client, &owner, group.id).await.unwrap();
        assert!(matches!(
            get_group(&client, group.id).await.unwrap_err(),
            GroupError::NotFound
        ));
    }

    #[tokio::test]
    async fn test_membership_add_remove_change_level() {
        let (client, _tmp) = make_client().await;
        let admin_id = Uuid::new_v4();
        let member_id = Uuid::new_v4();
        insert_user(&client, admin_id, "admin").await;
        insert_user(&client, member_id, "member").await;
        let admin = auth(admin_id, true);

        let group = create_group(&client, &admin, "members", false, false, "", "", None)
            .await
            .unwrap();

        let gm = add_member(
            &client,
            &admin,
            group.id,
            Some(member_id),
            None,
            GroupLevel::ReadOnly,
        )
        .await
        .unwrap();
        assert_eq!(gm.level, "read-only");

        // Duplicate enrollment rejected.
        let err = add_member(
            &client,
            &admin,
            group.id,
            Some(member_id),
            None,
            GroupLevel::Admin,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, GroupError::BadRequest(_)));

        // Level enum validation via change_level.
        let changed = change_level(&client, &admin, group.id, gm.id, GroupLevel::Maintainer)
            .await
            .unwrap();
        assert_eq!(changed.level, "maintainer");

        // The creating admin is an implicit admin-level member row.
        let members = list_members(&client, group.id).await.unwrap();
        assert_eq!(members.len(), 2, "creator + added member");

        remove_member(&client, &admin, group.id, gm.id)
            .await
            .unwrap();
        let members = list_members(&client, group.id).await.unwrap();
        assert_eq!(members.len(), 1, "only the creator-admin remains");
    }

    #[tokio::test]
    async fn test_membership_requires_exactly_one_target() {
        let (client, _tmp) = make_client().await;
        let admin_id = Uuid::new_v4();
        insert_user(&client, admin_id, "admin").await;
        let admin = auth(admin_id, true);
        let group = create_group(&client, &admin, "one-target", false, false, "", "", None)
            .await
            .unwrap();

        let err = add_member(&client, &admin, group.id, None, None, GroupLevel::Admin)
            .await
            .unwrap_err();
        assert!(matches!(err, GroupError::BadRequest(_)));

        let err = add_member(
            &client,
            &admin,
            group.id,
            Some(Uuid::new_v4()),
            Some(Uuid::new_v4()),
            GroupLevel::Admin,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, GroupError::BadRequest(_)));
    }

    #[tokio::test]
    async fn test_group_level_from_str_validation() {
        assert_eq!(
            "read-only".parse::<GroupLevel>().unwrap(),
            GroupLevel::ReadOnly
        );
        assert_eq!(
            "contributor".parse::<GroupLevel>().unwrap(),
            GroupLevel::Contributor
        );
        assert_eq!(
            "maintainer".parse::<GroupLevel>().unwrap(),
            GroupLevel::Maintainer
        );
        assert_eq!("admin".parse::<GroupLevel>().unwrap(), GroupLevel::Admin);
        assert!("superuser".parse::<GroupLevel>().is_err());
    }

    #[tokio::test]
    async fn test_owner_is_implicit_admin_member() {
        let (client, _tmp) = make_client().await;
        let admin_id = Uuid::new_v4();
        let owner_id = Uuid::new_v4();
        insert_user(&client, admin_id, "admin").await;
        insert_user(&client, owner_id, "owner").await;
        let admin = auth(admin_id, true);

        let group = create_group(
            &client,
            &admin,
            "owner-implicit",
            false,
            false,
            "",
            "",
            Some(owner_id),
        )
        .await
        .unwrap();
        assert_eq!(
            is_member(&client, &group.id, owner_id).await.unwrap(),
            Some(GroupLevel::Admin)
        );
    }

    #[tokio::test]
    async fn test_resolve_members_groups_of_groups_with_cycle_guard() {
        let (client, _tmp) = make_client().await;
        let admin_id = Uuid::new_v4();
        let user_a = Uuid::new_v4();
        let user_b = Uuid::new_v4();
        insert_user(&client, admin_id, "admin").await;
        insert_user(&client, user_a, "user-a").await;
        insert_user(&client, user_b, "user-b").await;
        let admin = auth(admin_id, true);

        let parent = create_group(&client, &admin, "parent", false, false, "", "", None)
            .await
            .unwrap();
        let child = create_group(&client, &admin, "child", false, false, "", "", None)
            .await
            .unwrap();
        let grandchild = create_group(&client, &admin, "grandchild", false, false, "", "", None)
            .await
            .unwrap();

        add_member(
            &client,
            &admin,
            child.id,
            Some(user_a),
            None,
            GroupLevel::ReadOnly,
        )
        .await
        .unwrap();
        add_member(
            &client,
            &admin,
            grandchild.id,
            Some(user_b),
            None,
            GroupLevel::ReadOnly,
        )
        .await
        .unwrap();

        // parent ⊇ child ⊇ grandchild
        add_member(
            &client,
            &admin,
            parent.id,
            None,
            Some(child.id),
            GroupLevel::Contributor,
        )
        .await
        .unwrap();
        add_member(
            &client,
            &admin,
            child.id,
            None,
            Some(grandchild.id),
            GroupLevel::ReadOnly,
        )
        .await
        .unwrap();

        let members = resolve_members(&client, &parent.id).await.unwrap();
        assert!(members.contains(&user_a), "user_a via child subgroup");
        assert!(members.contains(&user_b), "user_b via grandchild roll-up");
        assert!(members.contains(&admin_id), "owner implicitly included");
        assert!(members.contains(&admin_id));

        // Subgroup member is capped by the subgroup's level in the parent:
        // user_a is read-only in child and child is contributor in parent.
        assert_eq!(
            is_member(&client, &parent.id, user_a).await.unwrap(),
            Some(GroupLevel::ReadOnly)
        );

        // Cycle: adding parent as a subgroup of grandchild must be rejected.
        let err = add_member(
            &client,
            &admin,
            grandchild.id,
            None,
            Some(parent.id),
            GroupLevel::Admin,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, GroupError::BadRequest(_)));

        // Adding a group to itself is rejected.
        let err = add_member(
            &client,
            &admin,
            parent.id,
            None,
            Some(parent.id),
            GroupLevel::Admin,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, GroupError::BadRequest(_)));

        // A cycle accidentally present must not hang resolution: insert one
        // directly into the DB, then resolve.
        client
            .execute(
                "INSERT INTO group_members (id, group_id, user_id, member_group_id, level, created_at) \
                 VALUES ($1, $2, NULL, $3, 'admin', $4)",
                hiqlite::params!(
                    Uuid::new_v4().to_string(),
                    child.id.to_string(),
                    parent.id.to_string(),
                    utc_now()
                ),
            )
            .await
            .unwrap();
        let members = resolve_members(&client, &parent.id).await.unwrap();
        assert!(members.contains(&user_a) || members.contains(&user_b));
    }

    #[tokio::test]
    async fn test_scope_flag_folds_members_from_user_scopes() {
        let (client, _tmp) = make_client().await;
        let admin_id = Uuid::new_v4();
        let scoped_user = Uuid::new_v4();
        let plain_user = Uuid::new_v4();
        insert_user(&client, admin_id, "admin").await;
        insert_user(&client, scoped_user, "scoped").await;
        insert_user(&client, plain_user, "plain").await;

        client
            .execute(
                "UPDATE users SET scopes = $1 WHERE id = $2",
                hiqlite::params!("openid profile billing:read", scoped_user.to_string()),
            )
            .await
            .unwrap();

        let admin = auth(admin_id, true);
        let group = create_group(
            &client,
            &admin,
            "billing:read",
            false,
            true,
            "Billing readers",
            "",
            None,
        )
        .await
        .unwrap();
        assert!(group.is_oauth_scope);

        // The user holding the matching scope is a (read-only) member.
        assert_eq!(
            is_member(&client, &group.id, scoped_user).await.unwrap(),
            Some(GroupLevel::ReadOnly)
        );
        // A user without the scope is not.
        assert_eq!(
            is_member(&client, &group.id, plain_user).await.unwrap(),
            None
        );
        assert!(resolve_members(&client, &group.id)
            .await
            .unwrap()
            .contains(&scoped_user));
    }

    #[tokio::test]
    async fn test_groups_for_user_and_has_group_access() {
        let (client, _tmp) = make_client().await;
        let admin_id = Uuid::new_v4();
        let member_id = Uuid::new_v4();
        insert_user(&client, admin_id, "admin").await;
        insert_user(&client, member_id, "member").await;
        let admin = auth(admin_id, true);
        let member = auth(member_id, false);

        let group = create_group(&client, &admin, "access", false, false, "", "", None)
            .await
            .unwrap();
        add_member(
            &client,
            &admin,
            group.id,
            Some(member_id),
            None,
            GroupLevel::ReadOnly,
        )
        .await
        .unwrap();

        // Server admin implicitly holds admin on every group.
        assert_eq!(
            has_group_access(&client, &group.id, &admin).await.unwrap(),
            Some(GroupLevel::Admin)
        );
        assert_eq!(
            has_group_access(&client, &group.id, &member).await.unwrap(),
            Some(GroupLevel::ReadOnly)
        );

        let groups = groups_for_user(&client, member_id).await.unwrap();
        assert!(groups.contains(&group.id));

        let stranger = auth(Uuid::new_v4(), false);
        assert_eq!(
            has_group_access(&client, &group.id, &stranger)
                .await
                .unwrap(),
            None
        );
    }
}
