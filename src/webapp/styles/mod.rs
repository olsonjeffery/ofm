pub mod app {
    use leptos_styling::style_sheet;

    style_sheet!(app_styles, "src/webapp/styles/app.css", "app_styles");
}
pub mod bulmaswatch {
    use leptos_styling::style_sheet;

    style_sheet!(
        bulma_styles,
        "src/webapp/styles/bulmaswatch.min.css",
        "bulma_styles"
    );
}
