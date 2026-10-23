//! Shared page shell: head, navigation rail or bottom bar, and the vendored scripts.

use maud::{DOCTYPE, Markup, html};

use crate::assets::asset_url;
use crate::components::{bottom_bar, rail};

/// The navigation entry a page belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Tasks,
    Projects,
    Personas,
    Credentials,
    /// Pages outside the navigation (login errors, 403).
    None,
}

/// Which screen of the task workspace a phone shows; larger screens show both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    List,
    Task,
}

impl View {
    const fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Task => "task",
        }
    }
}

/// htmx keeps the `open` state of `<details>` (tool calls) when it morphs an updated element.
const HTMX_CONFIG: &str = r#"{"morphIgnore":["data-htmx-powered","open","style","aria-current"]}"#;

/// Renders a full page of the usual width.
pub fn layout(title: &str, nav: Nav, body: Markup) -> Markup {
    document(title, nav, None, "/tasks/compose", body)
}

/// Renders a full-width page of the task workspace. `compose_href` opens the composer with the
/// current project filter as default.
pub(crate) fn workspace_layout(
    title: &str,
    view: View,
    compose_href: &str,
    body: Markup,
) -> Markup {
    document(title, Nav::Tasks, Some(view), compose_href, body)
}

fn document(title: &str, nav: Nav, view: Option<View>, compose_href: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="color-scheme" content="light dark";
                meta name="htmx-config" content=(HTMX_CONFIG);
                title { (title) " - Dark Factory" }
                link rel="stylesheet" href=(asset_url("app.css"));
                link rel="preload" as="font" type="font/woff2" crossorigin
                    href=(asset_url("fonts/inter-latin-wght-normal.woff2"));
                script src=(asset_url("vendor/htmx.min.js")) defer {}
                script src=(asset_url("vendor/hx-sse.min.js")) defer {}
                script src=(asset_url("vendor/alpine.min.js")) defer {}
            }
            // On a phone, browser Back must show the screen of the URL again: the row click set
            // `data-view` to `task`, and history navigation does not undo that.
            body class="bg-app font-sans text-ink antialiased" data-view=[view.map(View::as_str)]
                x-data=[view.map(|_| "")]
                "x-on:popstate.window"=[view.map(|_| "document.body.dataset.view = /^\\/tasks\\/[A-Z]/.test(location.pathname) ? 'task' : 'list'")] {
                div class="flex min-h-dvh" {
                    @if nav != Nav::None { (rail(nav, compose_href)) }
                    main class={"min-w-0 flex-1 " (if view.is_some() { "" } else { "mx-auto w-full max-w-5xl px-4 py-4 pb-20 lg:py-6 lg:pb-6" })} {
                        (body)
                    }
                }
                @if nav != Nav::None { (bottom_bar(nav, compose_href)) }
                div id="composer-slot" {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nav_html(page: &str, id: &str) -> String {
        let start = page.find(&format!("id=\"{id}\"")).expect(id);
        let end = page[start..].find("</nav>").expect("nav end");
        page[start..start + end].to_string()
    }

    #[test]
    fn mobile_menu_lists_tasks_but_no_configuration_pages() {
        let page = layout("Tasks", Nav::Tasks, html! { p { "x" } }).into_string();
        let mobile = nav_html(&page, "mobile-menu");
        assert!(mobile.contains("href=\"/tasks\""));
        assert!(mobile.contains("min-h-11"), "{mobile}");
        for hidden in ["/projects", "/personas", "/credentials"] {
            assert!(!mobile.contains(hidden), "{hidden} in mobile menu");
        }
        let desktop = nav_html(&page, "desktop-menu");
        for shown in ["/tasks", "/projects", "/personas", "/credentials"] {
            assert!(desktop.contains(shown), "{shown} missing on desktop");
        }
    }

    #[test]
    fn rail_on_the_task_page_marks_tasks_current() {
        let page = layout("Tasks", Nav::Tasks, html! {}).into_string();
        let desktop = nav_html(&page, "desktop-menu");
        assert_eq!(
            desktop.matches("aria-current=\"page\"").count(),
            1,
            "{desktop}"
        );
        assert!(page.contains("aria-label=\"New task\"") && page.contains("hx-post=\"/logout\""));
    }

    #[test]
    fn pages_outside_the_navigation_have_no_menu() {
        let page = layout("Forbidden", Nav::None, html! {}).into_string();
        assert!(!page.contains("mobile-menu") && !page.contains("desktop-menu"));
        assert!(page.contains("<title>Forbidden - Dark Factory</title>"));
    }

    #[test]
    fn page_loads_vendored_scripts_stylesheet_and_fonts_with_hashes() {
        let page = layout("T", Nav::Tasks, html! {}).into_string();
        for asset in [
            "app.css",
            "vendor/htmx.min.js",
            "vendor/hx-sse.min.js",
            "vendor/alpine.min.js",
            "fonts/inter-latin-wght-normal.woff2",
        ] {
            assert!(page.contains(&format!("/assets/{asset}?v=")), "{asset}");
        }
    }

    #[test]
    fn theme_comes_from_css_only() {
        let page = layout("T", Nav::Tasks, html! {}).into_string();
        assert!(page.contains("name=\"color-scheme\" content=\"light dark\""));
        // The only inline attributes are Alpine/htmx ones; no script chooses a theme.
        assert!(!page.contains("<script>") && !page.contains("matchMedia"));
    }

    #[test]
    fn workspace_marks_the_view_for_phones() {
        let page = workspace_layout("T", View::Task, "/tasks/compose", html! {}).into_string();
        assert!(page.contains("data-view=\"task\""));
        assert!(
            page.contains("x-on:popstate.window=\"document.body.dataset.view"),
            "{page}"
        );
        assert!(
            layout("T", Nav::Tasks, html! {})
                .into_string()
                .contains("<body class=\"bg-app font-sans text-ink antialiased\">")
        );
    }

    #[test]
    fn task_list_on_a_phone_has_the_bottom_bar_and_no_rail_in_view() {
        let page = layout("T", Nav::Tasks, html! {}).into_string();
        let bar = nav_html(&page, "mobile-menu");
        assert!(bar.contains("lg:hidden"), "bottom bar only below 1024 px");
        for label in ["Tasks", "Log out"] {
            assert!(bar.contains(label), "{label}");
        }
        assert!(bar.contains("aria-label=\"New task\""));
        // The rail is hidden below 1024 px.
        assert!(page.contains("class=\"sticky top-0 hidden h-dvh"), "{page}");
    }

    #[test]
    fn composer_slot_exists_for_the_new_task_action() {
        let page = layout("T", Nav::Tasks, html! {}).into_string();
        assert!(page.contains("id=\"composer-slot\""));
    }
}
