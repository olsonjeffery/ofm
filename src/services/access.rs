use hiqlite::Client;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::schema::{GroupLevel, Project, Task, UserModelConfig};
use crate::services::groups::{self, GroupError};

/// Central authorization chokepoint for user-owned resources.
///
/// A resource owned by `owner_id` is accessible to `requester` when they are
/// the owner, a server admin (implicit highest-level access everywhere), or a
/// member — at `min_level` or higher — of any group the owner belongs to
/// (resource → group linkage is modeled as the owner's group membership; see
/// spec/extra/auth-and-multi-user.md).
pub async fn has_resource_access(
    client: &Client,
    requester: &AuthUser,
    owner_id: Uuid,
    min_level: GroupLevel,
) -> Result<bool, GroupError> {
    if requester.user_id == owner_id || requester.is_admin {
        return Ok(true);
    }
    for group_id in groups::groups_for_user(client, owner_id).await? {
        if let Some(level) = groups::is_member(client, &group_id, requester.user_id).await? {
            if level >= min_level {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// `group_level_at_least` helper: true when `level` is present and ≥ `required`.
pub fn group_level_at_least(level: Option<GroupLevel>, required: GroupLevel) -> bool {
    level.is_some_and(|l| l >= required)
}

// ── Projects ─────────────────────────────────────────────────────────────────

pub async fn has_project_access(
    client: &Client,
    requester: &AuthUser,
    project: &Project,
) -> Result<bool, GroupError> {
    has_resource_access(client, requester, project.user_id, GroupLevel::ReadOnly).await
}

pub async fn has_project_write_access(
    client: &Client,
    requester: &AuthUser,
    project: &Project,
) -> Result<bool, GroupError> {
    has_resource_access(client, requester, project.user_id, GroupLevel::Contributor).await
}

/// Projects the requester can access (owned, or shared via group membership).
pub async fn list_accessible_projects(
    client: &Client,
    requester: &AuthUser,
) -> Result<Vec<Project>, GroupError> {
    let all = client
        .query_map::<Project, _>(
            "SELECT id, user_id, name, repo_folder_path, subproject_path, tags, created_at \
             FROM projects ORDER BY created_at DESC",
            hiqlite::params!(),
        )
        .await
        .map_err(|e| GroupError::Db(e.to_string()))?;
    let mut accessible = Vec::new();
    for project in all {
        if has_project_access(client, requester, &project).await? {
            accessible.push(project);
        }
    }
    Ok(accessible)
}

// ── Model Configurations ─────────────────────────────────────────────────────

pub async fn has_model_config_access(
    client: &Client,
    requester: &AuthUser,
    config: &UserModelConfig,
) -> Result<bool, GroupError> {
    has_resource_access(client, requester, config.user_id, GroupLevel::ReadOnly).await
}

pub async fn has_model_config_write_access(
    client: &Client,
    requester: &AuthUser,
    config: &UserModelConfig,
) -> Result<bool, GroupError> {
    has_resource_access(client, requester, config.user_id, GroupLevel::Contributor).await
}

/// Model configs (opencode harness only — the Model Configurations surface)
/// that the requester can access, owned or shared via group membership.
pub async fn list_accessible_model_configs(
    client: &Client,
    requester: &AuthUser,
) -> Result<Vec<UserModelConfig>, GroupError> {
    let all = client
        .query_map::<UserModelConfig, _>(
            "SELECT id, user_id, name, config_body, harness, created_at, updated_at \
             FROM user_model_configs WHERE harness = 'opencode' ORDER BY created_at",
            hiqlite::params!(),
        )
        .await
        .map_err(|e| GroupError::Db(e.to_string()))?;
    let mut accessible = Vec::new();
    for config in all {
        if has_model_config_access(client, requester, &config).await? {
            accessible.push(config);
        }
    }
    Ok(accessible)
}

// ── Task Flows ───────────────────────────────────────────────────────────────

pub async fn has_task_flow_access(
    client: &Client,
    requester: &AuthUser,
    task: &Task,
) -> Result<bool, GroupError> {
    has_resource_access(client, requester, task.user_id, GroupLevel::ReadOnly).await
}

pub async fn has_task_flow_write_access(
    client: &Client,
    requester: &AuthUser,
    task: &Task,
) -> Result<bool, GroupError> {
    has_resource_access(client, requester, task.user_id, GroupLevel::Contributor).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::{GroupLevel, Project};
    use crate::services::groups;

    fn auth(user_id: Uuid, is_admin: bool) -> AuthUser {
        AuthUser {
            user_id,
            username: "actor".into(),
            oidc_subject: None,
            is_admin,
            is_technical: false,
        }
    }

    async fn make_client() -> (hiqlite::Client, tempfile::TempDir) {
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

    async fn insert_user(client: &hiqlite::Client, id: Uuid, username: &str) {
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        client
            .execute(
                "INSERT INTO users (id, username, is_admin, is_active, created_at) VALUES ($1, $2, 0, 1, $3)",
                hiqlite::params!(id.to_string(), username, now),
            )
            .await
            .unwrap();
    }

    fn project(owner: Uuid) -> Project {
        Project {
            id: 1,
            user_id: owner,
            name: "p".into(),
            repo_folder_path: "/tmp/repo".into(),
            subproject_path: None,
            tags: vec![],
            created_at: chrono::Utc::now().naive_utc(),
        }
    }

    #[tokio::test]
    async fn test_owner_has_full_access() {
        let (client, _tmp) = make_client().await;
        let owner = Uuid::new_v4();
        insert_user(&client, owner, "owner").await;
        let auth = auth(owner, false);
        let p = project(owner);

        assert!(has_project_access(&client, &auth, &p).await.unwrap());
        assert!(has_project_write_access(&client, &auth, &p).await.unwrap());
    }

    #[tokio::test]
    async fn test_admin_has_implicit_access_everywhere() {
        let (client, _tmp) = make_client().await;
        let admin = Uuid::new_v4();
        let owner = Uuid::new_v4();
        insert_user(&client, admin, "admin").await;
        insert_user(&client, owner, "owner").await;
        let auth = auth(admin, true);
        let p = project(owner);

        assert!(has_project_access(&client, &auth, &p).await.unwrap());
        assert!(has_project_write_access(&client, &auth, &p).await.unwrap());
    }

    #[tokio::test]
    async fn test_group_membership_read_only_gates_writes() {
        let (client, _tmp) = make_client().await;
        let admin_id = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let member = Uuid::new_v4();
        insert_user(&client, admin_id, "admin").await;
        insert_user(&client, owner, "owner").await;
        insert_user(&client, member, "member").await;
        let admin = auth(admin_id, true);

        // owner and member share a group; member is read-only.
        let group =
            groups::create_group(&client, &admin, "shared", false, false, "", "", Some(owner))
                .await
                .unwrap();
        groups::add_member(
            &client,
            &admin,
            group.id,
            Some(member),
            None,
            GroupLevel::ReadOnly,
        )
        .await
        .unwrap();

        let p = project(owner);
        let member_auth = auth(member, false);

        assert!(has_project_access(&client, &member_auth, &p).await.unwrap());
        assert!(!has_project_write_access(&client, &member_auth, &p)
            .await
            .unwrap());

        // A non-member cannot even read.
        let stranger = auth(Uuid::new_v4(), false);
        assert!(!has_project_access(&client, &stranger, &p).await.unwrap());
        assert!(!has_project_write_access(&client, &stranger, &p)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_contributor_allows_writes() {
        let (client, _tmp) = make_client().await;
        let admin_id = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let contributor = Uuid::new_v4();
        insert_user(&client, admin_id, "admin").await;
        insert_user(&client, owner, "owner").await;
        insert_user(&client, contributor, "contributor").await;
        let admin = auth(admin_id, true);

        let group = groups::create_group(
            &client,
            &admin,
            "write-shared",
            false,
            false,
            "",
            "",
            Some(owner),
        )
        .await
        .unwrap();
        groups::add_member(
            &client,
            &admin,
            group.id,
            Some(contributor),
            None,
            GroupLevel::Contributor,
        )
        .await
        .unwrap();

        let p = project(owner);
        let contributor_auth = auth(contributor, false);
        assert!(has_project_access(&client, &contributor_auth, &p)
            .await
            .unwrap());
        assert!(has_project_write_access(&client, &contributor_auth, &p)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_group_level_at_least_helper() {
        assert!(group_level_at_least(
            Some(GroupLevel::Admin),
            GroupLevel::ReadOnly
        ));
        assert!(group_level_at_least(
            Some(GroupLevel::Maintainer),
            GroupLevel::Contributor
        ));
        assert!(!group_level_at_least(
            Some(GroupLevel::ReadOnly),
            GroupLevel::Contributor
        ));
        assert!(!group_level_at_least(None, GroupLevel::ReadOnly));
    }

    #[tokio::test]
    async fn test_list_accessible_projects() {
        let (client, _tmp) = make_client().await;
        let admin_id = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let member = Uuid::new_v4();
        insert_user(&client, admin_id, "admin").await;
        insert_user(&client, owner, "owner").await;
        insert_user(&client, member, "member").await;
        let admin = auth(admin_id, true);

        // Owner creates a project; member is read-only in the shared group.
        let project_id: i64 = {
            let mut rows = client
                .query_raw(
                    "SELECT COALESCE(MAX(id), 0) + 1 AS next_id FROM projects",
                    hiqlite::params!(),
                )
                .await
                .unwrap();
            rows.first_mut()
                .map(|r| r.get::<i64>("next_id"))
                .unwrap_or(1)
        };
        client
            .execute(
                "INSERT INTO projects (id, user_id, name, repo_folder_path) VALUES ($1, $2, 'p', '/tmp/repo')",
                hiqlite::params!(project_id, owner.to_string()),
            )
            .await
            .unwrap();

        let group = groups::create_group(
            &client,
            &admin,
            "list-shared",
            false,
            false,
            "",
            "",
            Some(owner),
        )
        .await
        .unwrap();
        groups::add_member(
            &client,
            &admin,
            group.id,
            Some(member),
            None,
            GroupLevel::ReadOnly,
        )
        .await
        .unwrap();

        let member_auth = auth(member, false);
        let visible = list_accessible_projects(&client, &member_auth)
            .await
            .unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, project_id);

        let stranger = auth(Uuid::new_v4(), false);
        assert!(list_accessible_projects(&client, &stranger)
            .await
            .unwrap()
            .is_empty());
    }
}
