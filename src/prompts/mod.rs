//! Prompt Library render engine.
//!
//! Owns the 9-token allowlist (`{{taskId}}`, `{{projectName}}`, ...), token
//! extraction, non-destructive validation, tag grammar validation, and
//! rendering. Pure functions — no database access — so everything is
//! unit-testable in isolation. User-authored snippet/composite content is
//! validated against [`STANDARD_TOKENS`]; static templates are exempt because
//! they use extra, non-standard tokens (e.g. `{{planTemplateContent}}`).
//!
//! Rendering is additive and non-destructive: an unresolvable token is left
//! literal rather than crashing the run.

pub const STANDARD_TOKENS: [&str; 9] = [
    "taskId",
    "projectId",
    "taskDocPath",
    "taskWorktreePath",
    "taskWorktreeBranch",
    "projectDefaultBranch",
    "projectName",
    "taskName",
    "tags",
];

/// The 9 standard variable substitutions available to rendered prompts.
#[derive(Debug, Clone, Default)]
pub struct PromptVars {
    pub task_id: String,
    pub project_id: String,
    pub task_doc_path: String,
    pub task_worktree_path: String,
    pub task_worktree_branch: String,
    pub project_default_branch: String,
    pub project_name: String,
    pub task_name: String,
    /// Comma-space joined project tags.
    pub tags: String,
}

impl PromptVars {
    pub fn lookup(&self, token: &str) -> Option<&str> {
        match token {
            "taskId" => Some(&self.task_id),
            "projectId" => Some(&self.project_id),
            "taskDocPath" => Some(&self.task_doc_path),
            "taskWorktreePath" => Some(&self.task_worktree_path),
            "taskWorktreeBranch" => Some(&self.task_worktree_branch),
            "projectDefaultBranch" => Some(&self.project_default_branch),
            "projectName" => Some(&self.project_name),
            "taskName" => Some(&self.task_name),
            "tags" => Some(&self.tags),
            _ => None,
        }
    }
}

/// Parse a `{{token}}` placeholder starting at `i` (where `bytes[i..i+2]` is
/// `{{`), returning the token text when well formed and the index the caller
/// should resume scanning from. A token is a run of `[a-zA-Z0-9_]` inside
/// double curly braces with optional surrounding whitespace.
fn scan_token(bytes: &[u8], i: usize) -> (Option<String>, usize) {
    let mut j = i + 2;
    // skip leading whitespace inside the braces
    while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
        j += 1;
    }
    let start = j;
    while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
        j += 1;
    }
    let end = j;
    // skip trailing whitespace, then require the closing `}}`
    let mut k = j;
    while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
        k += 1;
    }
    let valid = end > start && k + 1 < bytes.len() && bytes[k] == b'}' && bytes[k + 1] == b'}';
    let token = valid.then(|| String::from_utf8_lossy(&bytes[start..end]).into_owned());
    (token, k + 2)
}

/// Extract the unique `{{token}}` placeholders from `content`, preserving
/// first-occurrence order.
pub fn extract_tokens(content: &str) -> Vec<String> {
    let bytes = content.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            let (token, next) = scan_token(bytes, i);
            if let Some(token) = token {
                if !tokens.contains(&token) {
                    tokens.push(token);
                }
            }
            i = next;
        } else {
            i += 1;
        }
    }
    tokens
}

/// Non-destructive validation: returns the unknown tokens in `content` (tokens
/// not in [`STANDARD_TOKENS`]). An empty vec means the content is valid.
pub fn validate(content: &str) -> Vec<String> {
    extract_tokens(content)
        .into_iter()
        .filter(|t| !STANDARD_TOKENS.contains(&t.as_str()))
        .collect()
}

/// Dash-based-name tag grammar: `^[a-z0-9]+(-[a-z0-9]+)*$`.
pub fn validate_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.split('-').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// Render `content`, substituting every standard token with its variable value
/// (empty string when unset). Unknown tokens are left literal.
pub fn render(content: &str, vars: &PromptVars) -> String {
    let mut out = String::with_capacity(content.len());
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            let (token, next) = scan_token(bytes, i);
            if let Some(token) = token {
                match vars.lookup(&token) {
                    Some(value) => out.push_str(value),
                    None => {
                        out.push_str("{{");
                        out.push_str(&token);
                        out.push_str("}}");
                    }
                }
                i = next;
            } else {
                out.push(bytes[i] as char);
                i += 1;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> PromptVars {
        PromptVars {
            task_id: "7".into(),
            project_id: "3".into(),
            task_doc_path: "/archive/projects/3/tasks/task-7.md".into(),
            task_worktree_path: "/foot/worktrees/project-3/task-7".into(),
            task_worktree_branch: "task/7-my-task".into(),
            project_default_branch: "main".into(),
            project_name: "ofm".into(),
            task_name: "Prompt Library".into(),
            tags: "desktop-3d, graphics".into(),
        }
    }

    #[test]
    fn test_extract_tokens_known_unknown_mixed() {
        let content =
            "Hello {{projectName}} and {{bogus}} plus {{ taskId }} and {{projectName}} again";
        let tokens = extract_tokens(content);
        assert_eq!(tokens, vec!["projectName", "bogus", "taskId"]);
    }

    #[test]
    fn test_extract_tokens_no_tokens() {
        assert!(extract_tokens("no tokens here").is_empty());
        assert!(extract_tokens("{{}}").is_empty());
        assert!(extract_tokens("{{ }").is_empty());
    }

    #[test]
    fn test_validate_returns_only_unknown() {
        assert!(validate("a {{taskId}} b {{projectName}} c").is_empty());
        assert_eq!(
            validate("{{taskId}} {{bogus}} {{taskName}} {{alsoBad}}"),
            vec!["bogus", "alsoBad"]
        );
    }

    #[test]
    fn test_validate_static_template_content_exempt() {
        // Static templates use extra tokens like {{planTemplateContent}}; the
        // validator is only applied to user-authored content, so it must not
        // flag these when used verbatim.
        assert_eq!(
            validate("{{planTemplateContent}} {{prContextLine}}"),
            vec!["planTemplateContent", "prContextLine"]
        );
    }

    #[test]
    fn test_render_substitutes_all_nine_tokens() {
        let content = concat!(
            "task={{taskId}} project={{projectId}} doc={{taskDocPath}} ",
            "wt={{taskWorktreePath}} branch={{taskWorktreeBranch}} ",
            "base={{projectDefaultBranch}} name={{projectName}} ",
            "taskName={{taskName}} tags={{tags}}"
        );
        let out = render(content, &vars());
        assert_eq!(
            out,
            "task=7 project=3 doc=/archive/projects/3/tasks/task-7.md \
             wt=/foot/worktrees/project-3/task-7 branch=task/7-my-task \
             base=main name=ofm taskName=Prompt Library tags=desktop-3d, graphics"
        );
    }

    #[test]
    fn test_render_leaves_unknown_tokens_literal() {
        let out = render("{{taskId}} {{bogus}} {{taskName}}", &vars());
        assert_eq!(out, "7 {{bogus}} Prompt Library");
    }

    #[test]
    fn test_render_unset_token_becomes_empty() {
        let vars = PromptVars::default();
        assert_eq!(render("a {{projectName}} b", &vars), "a  b");
    }

    #[test]
    fn test_render_whitespace_inside_braces() {
        let out = render("{{  taskId }}", &vars());
        assert_eq!(out, "7");
    }

    #[test]
    fn test_validate_tag_grammar() {
        assert!(validate_tag("desktop-3d-graphics"));
        assert!(validate_tag("single"));
        assert!(validate_tag("a-b-c1"));
        assert!(validate_tag("2024"));
        assert!(!validate_tag("Desktop 3D"));
        assert!(!validate_tag(""));
        assert!(!validate_tag("-leading"));
        assert!(!validate_tag("trailing-"));
        assert!(!validate_tag("UPPER"));
        assert!(!validate_tag("has space"));
        assert!(!validate_tag("a--b"));
    }
}
