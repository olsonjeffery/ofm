use leptos::prelude::*;

use crate::services::commits::CommitSummary;

#[derive(Clone)]
pub struct CommitListData {
    pub project_id: i64,
    pub task_id: i64,
    pub commits: Vec<CommitSummary>,
}

#[component]
pub fn CommitList(data: CommitListData) -> impl IntoView {
    let commit_count = data.commits.len();
    view! {
        <div class="box commit-table">
            <div class="level is-mobile" style="margin-bottom:0.5rem">
                <div class="level-left">
                    <h2 class="title is-4">"Commits"</h2>
                </div>
                <div class="level-right">
                    <span class="tag is-grey is-light ml-1">{commit_count}</span>
                </div>
            </div>
            {if data.commits.is_empty() {
                view! {
                    <p class="has-text-grey commit-empty">"No commits yet."</p>
                }.into_any()
            } else {
                view! {
                    <table class="table is-fullwidth is-hoverable is-narrow commit-table">
                        <thead>
                            <tr>
                                <th>"OID"</th>
                                <th>"Message"</th>
                                <th>"Author"</th>
                                <th>"Date"</th>
                                <th>"Files"</th>
                            </tr>
                        </thead>
                        <tbody>
                            {data.commits.iter().map(move |commit| {
                                let href = format!(
                                    "/webapp/projects/{}/tasks/{}/commits/{}",
                                    data.project_id, data.task_id, commit.short_oid
                                );
                                let message_href = href.clone();
                                let date = commit.authored_time.format("%Y-%m-%d %H:%M").to_string();
                                let author = commit.author_name.clone();
                                view! {
                                    <tr>
                                        <td class="commit-oid"><a href=href title={commit.oid.to_string()}>{commit.short_oid.clone()}</a></td>
                                        <td class="commit-message"><a href=message_href>{commit.summary.clone()}</a></td>
                                        <td>{author}</td>
                                        <td class="commit-date">{date}</td>
                                        <td class="has-text-right">{commit.files_changed}</td>
                                    </tr>
                                }
                            }).collect::<Vec<_>>()}
                        </tbody>
                    </table>
                }.into_any()
            }}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, NaiveDateTime, Utc};
    use gix::ObjectId;

    fn make_commit(i: i64, summary: &str) -> CommitSummary {
        let oid = ObjectId::from_hex(b"deadbeefdeadbeefdeadbeefdeadbeefdeadbeef").unwrap();
        CommitSummary {
            oid,
            short_oid: "deadbeef".into(),
            summary: summary.into(),
            author_name: format!("Author {i}"),
            author_email: format!("author{i}@example.com"),
            authored_time: DateTime::<Utc>::from_naive_utc_and_offset(
                NaiveDateTime::parse_from_str("2024-06-01 12:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
                Utc,
            ),
            files_changed: 3,
        }
    }

    #[test]
    fn commit_list_empty_state() {
        let data = CommitListData {
            project_id: 1,
            task_id: 2,
            commits: vec![],
        };
        let html = leptos::view! { <CommitList data /> }.to_html();
        assert!(html.contains("Commits"));
        assert!(html.contains("No commits yet."));
        assert!(!html.contains("commit-oid"));
    }

    #[test]
    fn commit_list_renders_rows_with_links() {
        let data = CommitListData {
            project_id: 1,
            task_id: 2,
            commits: vec![make_commit(1, "Implement feature")],
        };
        let html = leptos::view! { <CommitList data /> }.to_html();
        assert!(html.contains("Implement feature"));
        assert!(html.contains("Author 1"));
        assert!(html.contains("2024-06-01"));
        assert!(html.contains("deadbeef"));
        assert!(html.contains("3"));
        assert!(
            html.contains(r#"href="/webapp/projects/1/tasks/2/commits/deadbeef""#),
            "row should link to the commit page"
        );
    }

    #[test]
    fn commit_list_renders_oldest_first_rows() {
        let first = make_commit(1, "First commit");
        let second = make_commit(2, "Second commit");
        let data = CommitListData {
            project_id: 1,
            task_id: 2,
            commits: vec![first, second],
        };
        let html = leptos::view! { <CommitList data /> }.to_html();
        let first_pos = html.find("First commit").unwrap();
        let second_pos = html.find("Second commit").unwrap();
        assert!(
            first_pos < second_pos,
            "oldest commit should render above the newest"
        );
    }

    #[test]
    fn commit_list_empty_state_no_table() {
        let data = CommitListData {
            project_id: 1,
            task_id: 2,
            commits: vec![],
        };
        let html = leptos::view! { <CommitList data /> }.to_html();
        assert!(!html.contains("<table"));
        assert!(!html.contains("<tbody>"));
    }
}
