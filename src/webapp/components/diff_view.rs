use leptos::prelude::*;
use similar::ChangeTag;

use crate::services::commits::{FileDiff, FileStatus};

/// Render one diff row: a full-width cell with a line-number gutter and the
/// line text, tinted by change type.
fn diff_cell(class: &'static str, lineno: Option<u32>, text: String) -> AnyView {
    view! {
        <td class=class>
            <span class="diff-gutter">{lineno.map(|n| n.to_string()).unwrap_or_default()}</span>
            <pre class="diff-line-content">{text}</pre>
        </td>
    }
    .into_any()
}

#[component]
pub fn DiffView(files: Vec<FileDiff>) -> impl IntoView {
    if files.is_empty() {
        return view! {
            <p class="has-text-grey diff-empty">"No file changes."</p>
        }
        .into_any();
    }

    files
        .into_iter()
        .map(|file| {
            let status = file.status.as_str();
            let (status_class, status_icon) = match file.status {
                FileStatus::Added => ("is-success", "plus"),
                FileStatus::Modified => ("is-info", "pencil"),
                FileStatus::Deleted => ("is-danger", "minus"),
                FileStatus::Renamed => ("is-warning", "arrow-right-bold"),
            };
            let stat = format!("+{} -{}", file.additions, file.deletions);
            view! {
                <details class="diff-file" open>
                    <summary class="diff-file-header">
                        <span class="icon is-small diff-toggle" aria-hidden="true">
                            <i class="mdi mdi-chevron-down"></i>
                        </span>
                        <span class="diff-file-path commit-oid">{file.path.clone()}</span>
                        <span class={format!("tag is-small {}", status_class)}>
                            <span class="icon is-small"><i class={format!("mdi mdi-{}", status_icon)}></i></span>
                            <span>{status}</span>
                        </span>
                        <span class="diff-stat has-text-grey">{stat}</span>
                    </summary>
                    <table class="diff-grid">
                        <tbody>
                            {file.lines.into_iter().map(|line| {
                                let (class, lineno) = match line.line_type {
                                    ChangeTag::Delete => ("diff-cell diff-del", line.old_lineno),
                                    ChangeTag::Insert => ("diff-cell diff-add", line.new_lineno),
                                    ChangeTag::Equal => ("diff-cell diff-ctx", line.old_lineno),
                                };
                                view! {
                                    <tr>
                                        {diff_cell(class, lineno, line.text)}
                                    </tr>
                                }
                            }).collect::<Vec<_>>()}
                        </tbody>
                    </table>
                </details>
            }
        })
        .collect::<Vec<_>>()
        .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::commits::DiffLine;
    use crate::services::commits::FileStatus;

    fn make_file_diff() -> FileDiff {
        FileDiff {
            path: "src/main.rs".into(),
            status: FileStatus::Modified,
            additions: 2,
            deletions: 1,
            lines: vec![
                DiffLine {
                    line_type: ChangeTag::Equal,
                    old_lineno: Some(1),
                    new_lineno: Some(1),
                    text: "use std::fs;\n".into(),
                },
                DiffLine {
                    line_type: ChangeTag::Delete,
                    old_lineno: Some(2),
                    new_lineno: None,
                    text: "let a = 1;\n".into(),
                },
                DiffLine {
                    line_type: ChangeTag::Insert,
                    old_lineno: None,
                    new_lineno: Some(2),
                    text: "let b = 2;\n".into(),
                },
            ],
        }
    }

    #[test]
    fn diff_view_empty_state() {
        let html = leptos::view! { <DiffView files=vec![] /> }.to_html();
        assert!(html.contains("No file changes."));
    }

    #[test]
    fn diff_view_renders_file_header() {
        let html = leptos::view! { <DiffView files=vec![make_file_diff()] /> }.to_html();
        assert!(html.contains("src/main.rs"));
        assert!(html.contains("Modified"));
        assert!(html.contains("+2 -1"));
        assert!(html.contains("mdi-pencil"));
    }

    #[test]
    fn diff_view_change_classes() {
        let html = leptos::view! { <DiffView files=vec![make_file_diff()] /> }.to_html();
        assert!(
            html.contains("diff-add"),
            "insert cell should be marked diff-add"
        );
        assert!(
            html.contains("diff-del"),
            "delete cell should be marked diff-del"
        );
        assert!(
            html.contains("diff-ctx"),
            "context lines should be marked diff-ctx"
        );
    }

    #[test]
    fn diff_view_gutters_and_cells() {
        let html = leptos::view! { <DiffView files=vec![make_file_diff()] /> }.to_html();
        // Context line: old gutter 1 appears exactly once (single stacked row).
        assert_eq!(
            html.matches("diff-gutter\">1<").count(),
            1,
            "context gutter should appear exactly once"
        );
        // Delete line carries old lineno 2 and Insert line carries new lineno 2,
        // so gutter 2 appears exactly twice (once per stacked row).
        assert_eq!(
            html.matches("diff-gutter\">2<").count(),
            2,
            "delete and insert gutters should each render once"
        );
        assert!(html.contains("let a = 1;"));
        assert!(html.contains("let b = 2;"));
    }

    #[test]
    fn diff_view_context_line_appears_once() {
        let file = FileDiff {
            path: "a.txt".into(),
            status: FileStatus::Modified,
            additions: 1,
            deletions: 1,
            lines: vec![DiffLine {
                line_type: ChangeTag::Equal,
                old_lineno: Some(1),
                new_lineno: Some(1),
                text: "let x = 1;\n".into(),
            }],
        };
        let html = leptos::view! { <DiffView files=vec![file] /> }.to_html();
        assert_eq!(
            html.matches("let x = 1;").count(),
            1,
            "context line text should render exactly once"
        );
        assert_eq!(
            html.matches("diff-cell diff-ctx").count(),
            1,
            "exactly one context cell should render"
        );
    }

    #[test]
    fn diff_view_removed_before_added() {
        let html = leptos::view! { <DiffView files=vec![make_file_diff()] /> }.to_html();
        let del_pos = html.find("let a = 1;").expect("deleted line should render");
        let add_pos = html.find("let b = 2;").expect("added line should render");
        assert!(
            del_pos < add_pos,
            "removed (red) line should appear above the added (green) line"
        );
    }

    #[test]
    fn diff_view_files_collapsible_by_default_open() {
        let html = leptos::view! { <DiffView files=vec![make_file_diff()] /> }.to_html();
        assert!(
            html.contains("<details open"),
            "each file diff should be a <details open> block"
        );
        assert!(
            html.contains("<summary class=\"diff-file-header\">"),
            "the file header should be the collapse <summary>"
        );
        assert!(
            html.contains("mdi-chevron-down"),
            "collapse toggle icon should render in the header"
        );
        assert!(
            html.contains("<table class=\"diff-grid\">"),
            "diff table should be present under the summary"
        );
    }

    #[test]
    fn diff_view_wraps_long_lines() {
        let mut file = make_file_diff();
        file.lines.push(DiffLine {
            line_type: ChangeTag::Equal,
            old_lineno: Some(3),
            new_lineno: Some(3),
            text: "let x = \"a very long line that will exceed the diff column width and must wrap\";\n"
                .into(),
        });
        let html = leptos::view! { <DiffView files=vec![file] /> }.to_html();
        assert!(
            html.contains("diff-line-content"),
            "diff lines should use the wrapping content class"
        );
        assert!(
            html.contains("must wrap"),
            "long line text should be present"
        );
    }
}
