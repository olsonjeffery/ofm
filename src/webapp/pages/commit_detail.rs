use leptos::prelude::*;

use crate::services::commits::CommitDiff;
use crate::webapp::components::diff_view::DiffView;

#[component]
pub fn CommitDetailPage(diff: Option<CommitDiff>, project_id: i64, task_id: i64) -> impl IntoView {
    let task_link = format!("/webapp/projects/{project_id}/tasks/{task_id}");

    match diff {
        None => view! {
            <section class="section">
                <div class="box">
                    <h2 class="title is-4">"Commit not found."</h2>
                    <p class="has-text-grey">"This commit could not be resolved in the task's worktree."</p>
                    <a class="button is-small is-light" style="margin-top:1rem" href=task_link>
                        <span class="icon is-small"><i class="mdi mdi-arrow-left"></i></span>
                        <span>"Back to task details"</span>
                    </a>
                </div>
            </section>
        }
        .into_any(),
        Some(commit) => {
            let short = crate::services::commits::short_oid(commit.oid);
            let date = commit.authored_time.format("%Y-%m-%d %H:%M").to_string();
            let file_count = commit.files.len();
            view! {
                <section class="section">
                    <div class="box commit-header">
                        <div class="level is-mobile">
                            <div class="level-left">
                                <h2 class="title is-4 commit-oid">{short}</h2>
                            </div>
                            <div class="level-right">
                                <a class="button is-small is-light" href=task_link>
                                    <span class="icon is-small"><i class="mdi mdi-arrow-left"></i></span>
                                    <span>"Back to task"</span>
                                </a>
                            </div>
                        </div>
                        <p class="commit-summary">{commit.summary.clone()}</p>
                        <p class="has-text-grey">
                            <span class="icon is-small"><i class="mdi mdi-account"></i></span>
                            <span>{commit.author_name.clone()}</span>
                            <span class="commit-author-email">"<" {commit.author_email.clone()} ">"</span>
                            <span class="commit-date"> {date}</span>
                        </p>
                        <p class="has-text-grey commit-file-count">
                            <span class="icon is-small"><i class="mdi mdi-file-document-outline"></i></span>
                            <span>{file_count}</span>
                            <span>" file(s) changed"</span>
                        </p>
                    </div>
                    <DiffView files=commit.files />
                </section>
            }
            .into_any()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::commits::{DiffLine, FileDiff, FileStatus};
    use similar::ChangeTag;

    fn make_commit_diff() -> CommitDiff {
        CommitDiff {
            oid: gix::ObjectId::from_hex(b"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap(),
            summary: "Implement feature".into(),
            author_name: "Jane Doe".into(),
            author_email: "jane@example.com".into(),
            authored_time: chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
                chrono::NaiveDateTime::parse_from_str("2024-06-01 12:00:00", "%Y-%m-%d %H:%M:%S")
                    .unwrap(),
                chrono::Utc,
            ),
            files: vec![FileDiff {
                path: "src/lib.rs".into(),
                status: FileStatus::Added,
                additions: 1,
                deletions: 0,
                lines: vec![DiffLine {
                    line_type: ChangeTag::Insert,
                    old_lineno: None,
                    new_lineno: Some(1),
                    text: "pub fn hi() {}\n".into(),
                }],
            }],
        }
    }

    #[test]
    fn commit_detail_none_renders_not_found() {
        let html =
            leptos::view! { <CommitDetailPage diff=None project_id=1 task_id=2 /> }.to_html();
        assert!(html.contains("Commit not found."));
        assert!(
            html.contains(r#"href="/webapp/projects/1/tasks/2""#),
            "not-found state should link back to the task detail page"
        );
    }

    #[test]
    fn commit_detail_renders_header_fields() {
        let html = leptos::view! {
            <CommitDetailPage diff=Some(make_commit_diff()) project_id=1 task_id=2 />
        }
        .to_html();
        assert!(html.contains("deadbeef"));
        assert!(html.contains("Implement feature"));
        assert!(html.contains("Jane Doe"));
        assert!(html.contains("jane@example.com"));
        assert!(html.contains("2024-06-01"));
        assert!(html.contains("src/lib.rs"));
        assert!(html.contains("mdi-file-document-outline"));
    }

    #[test]
    fn commit_detail_renders_diff_content() {
        let html = leptos::view! {
            <CommitDetailPage diff=Some(make_commit_diff()) project_id=1 task_id=2 />
        }
        .to_html();
        assert!(html.contains("pub fn hi() {}"));
        assert!(html.contains("diff-add"));
        assert!(html.contains("diff-grid"));
    }
}
