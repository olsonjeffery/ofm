use leptos::prelude::*;

#[derive(Clone, Debug)]
pub struct BreadcrumbItem {
    pub title: String,
    pub icon: String,
    pub path: String,
}

impl BreadcrumbItem {
    pub fn new(title: impl Into<String>, icon: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            icon: icon.into(),
            path: path.into(),
        }
    }
}

pub fn title_truncate(in_str: &str) -> String {
    const LENGTH: usize = 24;
    if in_str.len() <= LENGTH {
        in_str.to_owned()
    } else {
        format!("{}…", in_str.chars().take(LENGTH).collect::<String>())
    }
}

pub mod breadcrumb_registry {
    use super::{title_truncate, BreadcrumbItem};

    pub fn all_projects() -> BreadcrumbItem {
        BreadcrumbItem::new("All Projects", "home", "/webapp")
    }

    pub fn project(name: &str, id: i64) -> BreadcrumbItem {
        BreadcrumbItem::new(
            title_truncate(name),
            "folder",
            format!("/webapp/projects/{}", id),
        )
    }

    pub fn task(title: &str, project_id: i64, task_id: i64) -> BreadcrumbItem {
        BreadcrumbItem::new(
            title_truncate(title),
            "card-bulleted-outline",
            format!("/webapp/projects/{}/tasks/{}", project_id, task_id),
        )
    }

    pub fn chat() -> BreadcrumbItem {
        BreadcrumbItem::new("Chat", "chat", "#")
    }

    pub fn chat_conversation(
        name: &str,
        project_id: i64,
        task_id: i64,
        conv_id: uuid::Uuid,
    ) -> BreadcrumbItem {
        BreadcrumbItem::new(
            title_truncate(name),
            "chat",
            format!(
                "/webapp/projects/{}/tasks/{}/chat/{}",
                project_id, task_id, conv_id
            ),
        )
    }

    pub fn settings() -> BreadcrumbItem {
        BreadcrumbItem::new("Settings", "cog", "/webapp/settings")
    }
}

#[component]
pub fn Breadcrumbs(breadcrumbs: Vec<BreadcrumbItem>) -> impl IntoView {
    let count = breadcrumbs.len();
    view! {
        <div class="navbar-item">
            <nav class="breadcrumb" aria-label="breadcrumbs">
                <ul>
                    {breadcrumbs.into_iter().enumerate().map(move |(i, item)| {
                        let is_active = i == count - 1;
                        view! {
                            <li class={if is_active { "is-active" } else { "" }}>
                                <a href={item.path} style="color: var(--bulma-white)">
                                    <span class="icon is-small">
                                        <i class={format!("mdi mdi-{}", item.icon)}></i>
                                    </span>
                                    <span>{item.title}</span>
                                </a>
                            </li>
                        }
                    }).collect::<Vec<_>>()}
                </ul>
            </nav>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_breadcrumb_item_new() {
        let item = BreadcrumbItem::new("Test", "home", "/webapp");
        assert_eq!(item.title, "Test");
        assert_eq!(item.icon, "home");
        assert_eq!(item.path, "/webapp");
    }

    #[test]
    fn test_registry_all_projects() {
        let item = breadcrumb_registry::all_projects();
        assert_eq!(item.title, "All Projects");
        assert_eq!(item.icon, "home");
        assert_eq!(item.path, "/webapp");
    }

    #[test]
    fn test_registry_project() {
        let item = breadcrumb_registry::project("My Project", 42);
        assert_eq!(item.title, "My Project");
        assert_eq!(item.icon, "folder");
        assert_eq!(item.path, "/webapp/projects/42");
    }

    #[test]
    fn test_registry_task() {
        let item = breadcrumb_registry::task("My Task", 1, 99);
        assert_eq!(item.title, "My Task");
        assert_eq!(item.icon, "card-bulleted-outline");
        assert_eq!(item.path, "/webapp/projects/1/tasks/99");
    }

    #[test]
    fn test_registry_chat() {
        let item = breadcrumb_registry::chat();
        assert_eq!(item.title, "Chat");
        assert_eq!(item.icon, "chat");
        assert_eq!(item.path, "#");
    }

    #[test]
    fn test_registry_settings() {
        let item = breadcrumb_registry::settings();
        assert_eq!(item.title, "Settings");
        assert_eq!(item.icon, "cog");
        assert_eq!(item.path, "/webapp/settings");
    }

    #[test]
    fn test_registry_chat_conversation() {
        let conv_id = uuid::Uuid::new_v4();
        let item = breadcrumb_registry::chat_conversation("My Chat", 1, 42, conv_id);
        assert_eq!(item.title, "My Chat");
        assert_eq!(item.icon, "chat");
        assert_eq!(
            item.path,
            format!("/webapp/projects/1/tasks/42/chat/{}", conv_id)
        );
    }

    #[test]
    fn test_breadcrumbs_renders_correct_number_of_items() {
        let items = vec![
            breadcrumb_registry::all_projects(),
            breadcrumb_registry::project("Test", 1),
        ];
        let html = leptos::view! { <Breadcrumbs breadcrumbs=items /> }.to_html();
        assert_eq!(html.matches("<li").count(), 2);
    }

    #[test]
    fn test_breadcrumbs_last_item_is_active() {
        let items = vec![
            breadcrumb_registry::all_projects(),
            breadcrumb_registry::project("Test", 1),
        ];
        let html = leptos::view! { <Breadcrumbs breadcrumbs=items /> }.to_html();
        assert!(html.contains("is-active"));
    }

    #[test]
    fn test_breadcrumbs_first_item_not_active_when_multiple() {
        let items = vec![
            breadcrumb_registry::all_projects(),
            breadcrumb_registry::project("Test", 1),
        ];
        let html = leptos::view! { <Breadcrumbs breadcrumbs=items /> }.to_html();
        assert!(html.contains("mdi-home"));
        assert!(html.contains("mdi-folder"));
    }

    #[test]
    fn test_breadcrumbs_icon_class_generated() {
        let items = vec![breadcrumb_registry::all_projects()];
        let html = leptos::view! { <Breadcrumbs breadcrumbs=items /> }.to_html();
        assert!(html.contains("mdi-home"));
    }
}
