use hiqlite::Client;
use uuid::Uuid;

use crate::db::schema::{AgentType, Prompt, PromptAssignment, PromptKind, ScopeType};
use crate::prompts;

#[derive(Debug, thiserror::Error)]
pub enum PromptError {
    #[error("prompt not found")]
    NotFound,
    #[error("static prompts cannot be modified")]
    StaticImmutable,
    #[error("{0}")]
    BadRequest(String),
    #[error("validation failed: unknown tokens {unknown_tokens:?}, invalid tags {invalid_tags:?}")]
    Validation {
        unknown_tokens: Vec<String>,
        invalid_tags: Vec<String>,
    },
    #[error("database error: {0}")]
    Db(String),
}

fn utc_now() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn db_err(e: hiqlite::Error) -> PromptError {
    PromptError::Db(e.to_string())
}

fn is_no_rows(e: &hiqlite::Error) -> bool {
    e.to_string().contains("no rows returned")
}

fn map_get_err(e: hiqlite::Error) -> PromptError {
    if is_no_rows(&e) {
        PromptError::NotFound
    } else {
        db_err(e)
    }
}

/// Validate user-authored content against the standard token allowlist and the
/// dash-based-name tag grammar. Non-destructive — never mutates.
pub fn validate_prompt_content(content: &str, tags: &[String]) -> Result<(), PromptError> {
    let unknown_tokens = prompts::validate(content);
    let invalid_tags: Vec<String> = tags
        .iter()
        .filter(|t| !prompts::validate_tag(t))
        .cloned()
        .collect();
    if unknown_tokens.is_empty() && invalid_tags.is_empty() {
        Ok(())
    } else {
        Err(PromptError::Validation {
            unknown_tokens,
            invalid_tags,
        })
    }
}

/// All prompts visible in the caller's library: their own, globally shared
/// prompts, and the immutable static templates.
pub async fn list_prompts(client: &Client, user_id: &Uuid) -> Result<Vec<Prompt>, PromptError> {
    client
        .query_map::<Prompt, _>(
            "SELECT id, kind, title, content, tags, owner_user_id, is_static, is_shared, static_key, created_at, updated_at \
             FROM prompts \
             WHERE owner_user_id = $1 OR is_shared = 1 OR is_static = 1 ORDER BY created_at DESC",
            hiqlite::params!(user_id.to_string()),
        )
        .await
        .map_err(db_err)
}

pub async fn get_prompt(client: &Client, id: &Uuid) -> Result<Prompt, PromptError> {
    client
        .query_map_one::<Prompt, _>(
            "SELECT id, kind, title, content, tags, owner_user_id, is_static, is_shared, static_key, created_at, updated_at FROM prompts WHERE id = $1",
            hiqlite::params!(id.to_string()),
        )
        .await
        .map_err(map_get_err)
}

/// The ordered child prompts of a composite (`parent_id`), by `position`.
pub async fn get_children(
    client: &Client,
    parent_id: &Uuid,
) -> Result<Vec<Prompt>, hiqlite::Error> {
    client
        .query_map::<Prompt, _>(
            "SELECT p.id, p.kind, p.title, p.content, p.tags, p.owner_user_id, p.is_static, p.is_shared, p.static_key, p.created_at, p.updated_at \
             FROM prompt_children c JOIN prompts p ON p.id = c.child_id \
             WHERE c.parent_id = $1 ORDER BY c.position",
            hiqlite::params!(parent_id.to_string()),
        )
        .await
}

/// Recursively flatten a composite prompt into a single content string, joining
/// each child's content with a `---` separator line (depth-first; nested
/// composites are flattened too). A bare snippet (or static template) returns
/// its own content. A visited set guards against composite reference cycles.
pub async fn flattened_content(client: &Client, prompt: &Prompt) -> Result<String, hiqlite::Error> {
    fn walk<'a>(
        client: &'a Client,
        prompt: &'a Prompt,
        visited: Vec<Uuid>,
    ) -> futures_util::future::BoxFuture<'a, Result<String, hiqlite::Error>> {
        Box::pin(async move {
            if prompt.kind != PromptKind::Composite {
                return Ok(prompt.content.clone());
            }
            if visited.contains(&prompt.id) {
                return Ok(String::new());
            }
            let mut visited = visited;
            visited.push(prompt.id);
            let children = get_children(client, &prompt.id).await?;
            let mut parts = Vec::with_capacity(children.len());
            for child in &children {
                parts.push(walk(client, child, visited.clone()).await?);
            }
            Ok(parts.join("\n---\n"))
        })
    }
    walk(client, prompt, Vec::new()).await
}

#[allow(clippy::too_many_arguments)]
pub async fn create_prompt(
    client: &Client,
    user_id: &Uuid,
    kind: PromptKind,
    title: &str,
    content: &str,
    tags: Vec<String>,
    is_shared: bool,
    children: Vec<Uuid>,
) -> Result<Prompt, PromptError> {
    if kind == PromptKind::Static {
        return Err(PromptError::BadRequest(
            "static prompts are seeded by the system".into(),
        ));
    }
    if title.trim().is_empty() {
        return Err(PromptError::BadRequest("title is required".into()));
    }
    validate_prompt_content(content, &tags)?;
    let tags_json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into());
    let now = utc_now();
    let id = Uuid::new_v4();
    client
        .execute(
            "INSERT INTO prompts (id, kind, title, content, tags, owner_user_id, is_static, is_shared, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, 0, $7, $8, $8)",
            hiqlite::params!(
                id.to_string(),
                kind.to_string(),
                title.trim(),
                content,
                &tags_json,
                user_id.to_string(),
                i64::from(is_shared),
                &now
            ),
        )
        .await
        .map_err(db_err)?;
    if kind == PromptKind::Composite {
        replace_children(client, &id, &children)
            .await
            .map_err(db_err)?;
    }
    get_prompt(client, &id).await
}

/// Update a user-owned prompt. Static prompts are rejected. Composites replace
/// their children rows (delete + reinsert) when `children` is `Some`.
#[allow(clippy::too_many_arguments)]
pub async fn update_prompt(
    client: &Client,
    id: &Uuid,
    title: &str,
    content: &str,
    tags: Vec<String>,
    is_shared: bool,
    children: Option<Vec<Uuid>>,
) -> Result<Prompt, PromptError> {
    let existing = get_prompt(client, id).await?;
    if existing.is_static {
        return Err(PromptError::StaticImmutable);
    }
    if title.trim().is_empty() {
        return Err(PromptError::BadRequest("title is required".into()));
    }
    validate_prompt_content(content, &tags)?;
    let tags_json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into());
    let now = utc_now();
    client
        .execute(
            "UPDATE prompts SET title = $1, content = $2, tags = $3, is_shared = $4, updated_at = $5 WHERE id = $6",
            hiqlite::params!(title.trim(), content, &tags_json, i64::from(is_shared), &now, id.to_string()),
        )
        .await
        .map_err(db_err)?;
    if existing.kind == PromptKind::Composite {
        if let Some(children) = children {
            replace_children(client, id, &children)
                .await
                .map_err(db_err)?;
        }
    }
    get_prompt(client, id).await
}

pub async fn delete_prompt(client: &Client, id: &Uuid) -> Result<bool, PromptError> {
    let existing = get_prompt(client, id).await?;
    if existing.is_static {
        return Err(PromptError::StaticImmutable);
    }
    let rows = client
        .execute(
            "DELETE FROM prompts WHERE id = $1",
            hiqlite::params!(id.to_string()),
        )
        .await
        .map_err(db_err)?;
    Ok(rows > 0)
}

/// Copy a prompt into the caller's library as a user-owned snippet/composite,
/// re-titling with " (copy)" and recursing children for composites.
pub async fn duplicate_prompt(
    client: &Client,
    user_id: &Uuid,
    source_id: &Uuid,
) -> Result<Prompt, PromptError> {
    let source = get_prompt(client, source_id).await?;
    let new_id = Uuid::new_v4();
    let title = format!("{} (copy)", source.title);
    let tags_json = serde_json::to_string(&source.tags).unwrap_or_else(|_| "[]".into());
    let now = utc_now();
    let kind = if source.kind == PromptKind::Static {
        PromptKind::Snippet
    } else {
        source.kind
    };
    client
        .execute(
            "INSERT INTO prompts (id, kind, title, content, tags, owner_user_id, is_static, is_shared, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, 0, 0, $7, $7)",
            hiqlite::params!(
                new_id.to_string(),
                kind.to_string(),
                title,
                source.content,
                &tags_json,
                user_id.to_string(),
                &now
            ),
        )
        .await
        .map_err(db_err)?;

    if source.kind == PromptKind::Composite {
        let children = get_children(client, source_id).await.map_err(db_err)?;
        let child_ids: Vec<Uuid> = children.iter().map(|c| c.id).collect();
        replace_children(client, &new_id, &child_ids)
            .await
            .map_err(db_err)?;
    }
    get_prompt(client, &new_id).await
}

async fn replace_children(
    client: &Client,
    parent_id: &Uuid,
    child_ids: &[Uuid],
) -> Result<(), hiqlite::Error> {
    client
        .execute(
            "DELETE FROM prompt_children WHERE parent_id = $1",
            hiqlite::params!(parent_id.to_string()),
        )
        .await?;
    for (position, child_id) in child_ids.iter().enumerate() {
        client
            .execute(
                "INSERT INTO prompt_children (parent_id, child_id, position) VALUES ($1, $2, $3)",
                hiqlite::params!(parent_id.to_string(), child_id.to_string(), position as i64),
            )
            .await?;
    }
    Ok(())
}

/// Upsert a designation, keyed on `(agent_type, scope_type, project_id)`.
pub async fn upsert_assignment(
    client: &Client,
    prompt_id: &Uuid,
    agent_type: &AgentType,
    scope_type: &ScopeType,
    project_id: Option<i64>,
) -> Result<PromptAssignment, PromptError> {
    let now = utc_now();
    let existing = client
        .query_map_one::<PromptAssignment, _>(
            "SELECT id, prompt_id, agent_type, scope_type, project_id, created_at \
             FROM prompt_assignments \
             WHERE agent_type = $1 AND scope_type = $2 AND COALESCE(project_id, -1) = COALESCE($3, -1)",
            hiqlite::params!(agent_type.to_string(), scope_type.to_string(), project_id),
        )
        .await;
    match existing {
        Ok(assignment) => {
            client
                .execute(
                    "UPDATE prompt_assignments SET prompt_id = $1 WHERE id = $2",
                    hiqlite::params!(prompt_id.to_string(), assignment.id.to_string()),
                )
                .await
                .map_err(db_err)?;
            get_assignment(client, &assignment.id).await
        }
        Err(_) => {
            let id = Uuid::new_v4();
            client
                .execute(
                    "INSERT INTO prompt_assignments (id, prompt_id, agent_type, scope_type, project_id, created_at) \
                     VALUES ($1, $2, $3, $4, $5, $6)",
                    hiqlite::params!(
                        id.to_string(),
                        prompt_id.to_string(),
                        agent_type.to_string(),
                        scope_type.to_string(),
                        project_id,
                        &now
                    ),
                )
                .await
                .map_err(db_err)?;
            get_assignment(client, &id).await
        }
    }
}

async fn get_assignment(client: &Client, id: &Uuid) -> Result<PromptAssignment, PromptError> {
    client
        .query_map_one::<PromptAssignment, _>(
            "SELECT id, prompt_id, agent_type, scope_type, project_id, created_at FROM prompt_assignments WHERE id = $1",
            hiqlite::params!(id.to_string()),
        )
        .await
        .map_err(map_get_err)
}

/// Global assignments plus project assignments for `project_id` (when given).
pub async fn list_assignments(
    client: &Client,
    project_id: Option<i64>,
) -> Result<Vec<PromptAssignment>, hiqlite::Error> {
    client
        .query_map::<PromptAssignment, _>(
            "SELECT id, prompt_id, agent_type, scope_type, project_id, created_at \
             FROM prompt_assignments WHERE scope_type = 'global' OR project_id = $1 \
             ORDER BY scope_type, agent_type",
            hiqlite::params!(project_id),
        )
        .await
}

/// All assignments designating `prompt_id` (across scopes and projects) — used
/// by the builder's "Designate" box.
pub async fn list_assignments_for_prompt(
    client: &Client,
    prompt_id: &Uuid,
) -> Result<Vec<PromptAssignment>, hiqlite::Error> {
    client
        .query_map::<PromptAssignment, _>(
            "SELECT id, prompt_id, agent_type, scope_type, project_id, created_at \
             FROM prompt_assignments WHERE prompt_id = $1 ORDER BY scope_type, agent_type",
            hiqlite::params!(prompt_id.to_string()),
        )
        .await
}

pub async fn delete_assignment(client: &Client, id: &Uuid) -> Result<bool, hiqlite::Error> {
    let rows = client
        .execute(
            "DELETE FROM prompt_assignments WHERE id = $1",
            hiqlite::params!(id.to_string()),
        )
        .await?;
    Ok(rows > 0)
}

/// Reject designations of prompts that are not renderable agent prompts: the
/// seeded `plan-template` static entry is a verbatim plan-output format, not an
/// agent phase prompt.
pub async fn validate_assignment_target(
    client: &Client,
    prompt_id: &Uuid,
) -> Result<(), PromptError> {
    let prompt = get_prompt(client, prompt_id).await?;
    if prompt.is_static && prompt.static_key.as_deref() == Some("plan-template") {
        return Err(PromptError::BadRequest(
            "the plan template cannot be designated as an agent prompt".into(),
        ));
    }
    Ok(())
}

/// Resolve the designated prompt for `agent_type` at `(user, project)`.
///
/// Walks the same scope-precedence order as
/// [`crate::providers::registry::resolve_harness_config`]: project-scoped
/// designations win over global ones. (`prompt_assignments` has no user
/// column, so the per-user scopes of the harness walk collapse into their
/// project/global projections; the argument is retained for parity with the
/// registry signature and future-proofing.) Returns `None` when no designation
/// matches — callers fall back to the stock template builder.
pub async fn resolve_prompt_for_agent(
    client: &Client,
    agent_type: &AgentType,
    user_id: &Uuid,
    project_id: i64,
) -> Result<Option<Prompt>, PromptError> {
    let _ = user_id;
    let scopes: [(ScopeType, Option<i64>); 2] = [
        (ScopeType::Project, Some(project_id)),
        (ScopeType::Global, None),
    ];
    for (scope, scope_project) in &scopes {
        let found = client
            .query_map_one::<PromptAssignment, _>(
                "SELECT id, prompt_id, agent_type, scope_type, project_id, created_at \
                 FROM prompt_assignments \
                 WHERE agent_type = $1 AND scope_type = $2 AND COALESCE(project_id, -1) = COALESCE($3, -1)",
                hiqlite::params!(agent_type.to_string(), scope.to_string(), *scope_project),
            )
            .await
            .ok();
        if let Some(assignment) = found {
            if let Ok(prompt) = get_prompt(client, &assignment.prompt_id).await {
                return Ok(Some(prompt));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use tempfile::TempDir;

    async fn make_client() -> (Client, TempDir) {
        let db_dir = TempDir::new().unwrap();
        let config = hiqlite::NodeConfig {
            node_id: 1,
            nodes: vec![hiqlite::Node {
                id: 1,
                addr_raft: "127.0.0.1:0".into(),
                addr_api: "127.0.0.1:0".into(),
            }],
            data_dir: db_dir.path().to_str().unwrap().to_string().into(),
            secret_raft: "test-raft-secret-12345".into(),
            secret_api: "test-api-secret-12345".into(),
            ..Default::default()
        };
        let client = hiqlite::start_node(config).await.unwrap();
        client.wait_until_healthy_db().await;
        db::run_migrations(&client).await.unwrap();
        db::ensure_static_prompts(&client).await.unwrap();
        (client, db_dir)
    }

    #[tokio::test]
    async fn test_crud_round_trip() {
        let (client, _tmp) = make_client().await;
        let user = db::ensure_default_user(&client).await.unwrap();

        let p = create_prompt(
            &client,
            &user,
            PromptKind::Snippet,
            "My snippet",
            "Project: {{projectName}}; tags: {{tags}}",
            vec!["desktop-3d".into()],
            false,
            vec![],
        )
        .await
        .unwrap();
        assert_eq!(p.title, "My snippet");
        assert_eq!(p.tags, vec!["desktop-3d".to_string()]);

        let got = get_prompt(&client, &p.id).await.unwrap();
        assert_eq!(got.kind, PromptKind::Snippet);

        let updated = update_prompt(
            &client,
            &p.id,
            "Renamed",
            "New {{taskId}}",
            vec![],
            true,
            None,
        )
        .await
        .unwrap();
        assert_eq!(updated.title, "Renamed");
        assert!(updated.is_shared);

        assert!(delete_prompt(&client, &p.id).await.unwrap());
        assert!(matches!(
            get_prompt(&client, &p.id).await,
            Err(PromptError::NotFound)
        ));
    }

    #[tokio::test]
    async fn test_validation_rejects_unknown_tokens_and_bad_tags() {
        let (client, _tmp) = make_client().await;
        let user = db::ensure_default_user(&client).await.unwrap();

        let err = create_prompt(
            &client,
            &user,
            PromptKind::Snippet,
            "Bad",
            "{{bogus}}",
            vec!["Hello World".into()],
            false,
            vec![],
        )
        .await
        .unwrap_err();
        match err {
            PromptError::Validation {
                unknown_tokens,
                invalid_tags,
            } => {
                assert_eq!(unknown_tokens, vec!["bogus".to_string()]);
                assert_eq!(invalid_tags, vec!["Hello World".to_string()]);
            }
            other => panic!("expected Validation error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_static_prompts_immutable() {
        let (client, _tmp) = make_client().await;
        let user = db::ensure_default_user(&client).await.unwrap();
        let library = list_prompts(&client, &user).await.unwrap();
        let static_prompt = library.iter().find(|p| p.is_static).unwrap();

        assert!(matches!(
            update_prompt(&client, &static_prompt.id, "x", "y", vec![], false, None).await,
            Err(PromptError::StaticImmutable)
        ));
        assert!(matches!(
            delete_prompt(&client, &static_prompt.id).await,
            Err(PromptError::StaticImmutable)
        ));
    }

    #[tokio::test]
    async fn test_duplicate_copies_and_retitles() {
        let (client, _tmp) = make_client().await;
        let user = db::ensure_default_user(&client).await.unwrap();

        let p = create_prompt(
            &client,
            &user,
            PromptKind::Snippet,
            "Original",
            "body {{taskId}}",
            vec![],
            false,
            vec![],
        )
        .await
        .unwrap();
        let copy = duplicate_prompt(&client, &user, &p.id).await.unwrap();
        assert_eq!(copy.title, "Original (copy)");
        assert_eq!(copy.content, "body {{taskId}}");
        assert!(!copy.is_static);
        assert_eq!(copy.owner_user_id, Some(user));

        let _ = update_prompt(&client, &p.id, "Original", "changed", vec![], false, None)
            .await
            .unwrap();
        let copy = get_prompt(&client, &copy.id).await.unwrap();
        assert_eq!(copy.content, "body {{taskId}}");
    }

    #[tokio::test]
    async fn test_composite_children_order_and_flatten() {
        let (client, _tmp) = make_client().await;
        let user = db::ensure_default_user(&client).await.unwrap();

        let a = create_prompt(
            &client,
            &user,
            PromptKind::Snippet,
            "A",
            "first block",
            vec![],
            false,
            vec![],
        )
        .await
        .unwrap();
        let b = create_prompt(
            &client,
            &user,
            PromptKind::Snippet,
            "B",
            "second block",
            vec![],
            false,
            vec![],
        )
        .await
        .unwrap();
        let nested = create_prompt(
            &client,
            &user,
            PromptKind::Composite,
            "Nested",
            "",
            vec![],
            false,
            vec![a.id],
        )
        .await
        .unwrap();
        let outer = create_prompt(
            &client,
            &user,
            PromptKind::Composite,
            "Outer",
            "",
            vec![],
            false,
            vec![nested.id, b.id],
        )
        .await
        .unwrap();

        let flat = flattened_content(&client, &outer).await.unwrap();
        assert_eq!(flat, "first block\n---\nsecond block");

        // Ordering is preserved via `position`.
        let children = get_children(&client, &outer.id).await.unwrap();
        assert_eq!(children[0].id, nested.id);
        assert_eq!(children[1].id, b.id);
    }

    #[tokio::test]
    async fn test_assignment_upsert_list_delete() {
        let (client, _tmp) = make_client().await;
        let user = db::ensure_default_user(&client).await.unwrap();

        let p = create_prompt(
            &client,
            &user,
            PromptKind::Snippet,
            "Review prompt",
            "review content",
            vec![],
            true,
            vec![],
        )
        .await
        .unwrap();

        let assignment =
            upsert_assignment(&client, &p.id, &AgentType::Review, &ScopeType::Global, None)
                .await
                .unwrap();
        assert_eq!(assignment.agent_type, AgentType::Review);
        assert_eq!(assignment.scope_type, ScopeType::Global);

        let assignments = list_assignments(&client, None).await.unwrap();
        assert_eq!(assignments.len(), 1);

        assert!(delete_assignment(&client, &assignment.id).await.unwrap());
        assert!(list_assignments(&client, None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_resolution_precedence_project_over_global() {
        let (client, _tmp) = make_client().await;
        let user = db::ensure_default_user(&client).await.unwrap();
        let project = crate::services::projects::create_project(
            &client,
            &user,
            "p",
            "/tmp/prompts-repo",
            None,
            &[],
        )
        .await
        .unwrap();

        let global = create_prompt(
            &client,
            &user,
            PromptKind::Snippet,
            "Global impl",
            "global {{taskId}}",
            vec![],
            true,
            vec![],
        )
        .await
        .unwrap();
        let project_prompt = create_prompt(
            &client,
            &user,
            PromptKind::Snippet,
            "Project impl",
            "project {{taskId}}",
            vec![],
            false,
            vec![],
        )
        .await
        .unwrap();

        upsert_assignment(
            &client,
            &global.id,
            &AgentType::Implementation,
            &ScopeType::Global,
            None,
        )
        .await
        .unwrap();
        upsert_assignment(
            &client,
            &project_prompt.id,
            &AgentType::Implementation,
            &ScopeType::Project,
            Some(project.id),
        )
        .await
        .unwrap();

        let resolved =
            resolve_prompt_for_agent(&client, &AgentType::Implementation, &user, project.id)
                .await
                .unwrap()
                .expect("project-scoped designation should win");
        assert_eq!(resolved.id, project_prompt.id);

        // A different project with no designation falls back to Global.
        let other_project = crate::services::projects::create_project(
            &client,
            &user,
            "other",
            "/tmp/other-repo",
            None,
            &[],
        )
        .await
        .unwrap();
        let resolved =
            resolve_prompt_for_agent(&client, &AgentType::Implementation, &user, other_project.id)
                .await
                .unwrap()
                .expect("global designation applies everywhere");
        assert_eq!(resolved.id, global.id);
    }

    #[tokio::test]
    async fn test_plan_template_not_assignable() {
        let (client, _tmp) = make_client().await;
        let library = list_prompts(&client, &db::ensure_default_user(&client).await.unwrap())
            .await
            .unwrap();
        let plan_template = library
            .iter()
            .find(|p| p.static_key.as_deref() == Some("plan-template"))
            .unwrap();
        assert!(matches!(
            validate_assignment_target(&client, &plan_template.id).await,
            Err(PromptError::BadRequest(_))
        ));
    }
}
