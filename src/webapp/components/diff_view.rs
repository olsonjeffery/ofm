use leptos::prelude::*;
use similar::ChangeTag;

use crate::services::commits::FileDiff;

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
                crate::services::commits::FileStatus::Added => ("is-success", "plus"),
                crate::services::commits::FileStatus::Modified => ("is-info", "pencil"),
                crate::services::commits::FileStatus::Deleted => ("is-danger", "minus"),
                crate::services::commits::FileStatus::Renamed => ("is-warning", "arrow-right-bold"),
            };
            let stat = format!("+{} -{}", file.additions, file.deletions);
            view! {
                <div class="diff-file">
                    <div class="diff-file-header">
                        <span class="diff-file-path commit-oid">{file.path.clone()}</span>
                        <span class={format!("tag is-small {}", status_class)}>
                            <span class="icon is-small"><i class={format!("mdi mdi-{}", status_icon)}></i></span>
                            <span>{status}</span>
                        </span>
                        <span class="diff-stat has-text-grey">{stat}</span>
                    </div>
                    <table class="diff-grid">
                        <tbody>
                            {file.lines.into_iter().map(|line| {
                                let show_old = line.line_type != ChangeTag::Insert;
                                let show_new = line.line_type != ChangeTag::Delete;
                                let old_cell_class = match line.line_type {
                                    ChangeTag::Delete => "diff-cell diff-del",
                                    _ => "diff-cell diff-ctx",
                                };
                                let new_cell_class = match line.line_type {
                                    ChangeTag::Insert => "diff-cell diff-add",
                                    _ => "diff-cell diff-ctx",
                                };
                                view! {
                                    <tr>
                                        {if show_old {
                                            view! {
                                                <td class=old_cell_class>
                                                    <span class="diff-gutter">{line.old_lineno.map(|n| n.to_string()).unwrap_or_default()}</span>
                                                    <pre>{line.text.clone()}</pre>
                                                </td>
                                            }.into_any()
                                        } else {
                                            view! { <td class="diff-cell diff-gap"></td> }.into_any()
                                        }}
                                        {if show_new {
                                            view! {
                                                <td class=new_cell_class>
                                                    <span class="diff-gutter">{line.new_lineno.map(|n| n.to_string()).unwrap_or_default()}</span>
                                                    <pre>{line.text.clone()}</pre>
                                                </td>
                                            }.into_any()
                                        } else {
                                            view! { <td class="diff-cell diff-gap"></td> }.into_any()
                                        }}
                                    </tr>
                                }
                            }).collect::<Vec<_>>()}
                        </tbody>
                    </table>
                </div>
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
    fn diff_view_two_column_classes() {
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
        // Context line: both gutters present.
        assert!(html.contains("diff-gutter\">1<"));
        // Delete line: old gutter 2 present, new side blank.
        assert!(html.contains("diff-gutter\">2<"));
        // Insert line: new gutter 2 present.
        assert!(html.contains("diff-gutter\">2<"));
        assert!(html.contains("let a = 1;"));
        assert!(html.contains("let b = 2;"));
    }
}
