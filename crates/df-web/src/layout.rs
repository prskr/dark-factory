//! Shared page shell: head, navigation and the vendored scripts.

use maud::{DOCTYPE, Markup, html};

use crate::assets::asset_url;

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

/// Desktop-only configuration links; they are not part of the mobile menu (ADR-0035).
const CONFIG_LINKS: [(Nav, &str, &str); 3] = [
    (Nav::Projects, "/projects", "Projects"),
    (Nav::Personas, "/personas", "Personas"),
    (Nav::Credentials, "/credentials", "Credentials"),
];

/// htmx keeps the `open` state of `<details>` (tool calls) when it morphs an updated element.
const HTMX_CONFIG: &str = r#"{"morphIgnore":["data-htmx-powered","open"]}"#;

/// Renders a full page.
pub fn layout(title: &str, nav: Nav, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" class="bg-slate-950" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="htmx-config" content=(HTMX_CONFIG);
                title { (title) " - Dark Factory" }
                link rel="stylesheet" href=(asset_url("app.css"));
                script src=(asset_url("vendor/htmx.min.js")) defer {}
                script src=(asset_url("vendor/hx-sse.min.js")) defer {}
                script src=(asset_url("vendor/alpine.min.js")) defer {}
            }
            body class="min-h-screen bg-slate-950 text-slate-100 antialiased" {
                @if nav != Nav::None { (header(nav)) }
                main class="mx-auto w-full max-w-5xl px-4 py-4 md:py-6" { (body) }
            }
        }
    }
}

fn header(active: Nav) -> Markup {
    let link = |target: Nav, href: &str, label: &str| {
        let class = if target == active {
            "inline-flex min-h-11 items-center rounded px-3 text-sm font-medium bg-slate-800 text-white"
        } else {
            "inline-flex min-h-11 items-center rounded px-3 text-sm font-medium text-slate-300 hover:bg-slate-800"
        };
        html! { a href=(href) class=(class) { (label) } }
    };
    html! {
        header class="border-b border-slate-800 bg-slate-900" {
            div class="mx-auto flex max-w-5xl items-center gap-2 px-4 py-2" {
                a href="/" class="mr-2 inline-flex min-h-11 items-center font-semibold text-white" { "Dark Factory" }
                nav id="desktop-menu" class="hidden items-center gap-1 md:flex" {
                    (link(Nav::Tasks, "/tasks", "Tasks"))
                    @for (target, href, label) in CONFIG_LINKS { (link(target, href, label)) }
                }
                nav id="mobile-menu" class="flex items-center gap-1 md:hidden" {
                    (link(Nav::Tasks, "/tasks", "Tasks"))
                }
                button type="button" class="ml-auto min-h-11 rounded px-3 text-sm text-slate-300 hover:bg-slate-800"
                    hx-post="/logout" { "Log out" }
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
        // Touch targets: the mobile link and the home link are as tall as the other controls.
        assert!(mobile.contains("min-h-11"), "{mobile}");
        assert!(
            page.contains("<a href=\"/\" class=\"mr-2 inline-flex min-h-11"),
            "home link"
        );
        for hidden in ["/projects", "/personas", "/credentials"] {
            assert!(!mobile.contains(hidden), "{hidden} in mobile menu");
        }
        let desktop = nav_html(&page, "desktop-menu");
        for shown in ["/tasks", "/projects", "/personas", "/credentials"] {
            assert!(desktop.contains(shown), "{shown} missing on desktop");
        }
    }

    #[test]
    fn pages_outside_the_navigation_have_no_menu() {
        let page = layout("Forbidden", Nav::None, html! {}).into_string();
        assert!(!page.contains("mobile-menu"));
        assert!(page.contains("<title>Forbidden - Dark Factory</title>"));
    }

    #[test]
    fn page_loads_vendored_scripts_and_stylesheet_with_hashes() {
        let page = layout("T", Nav::Tasks, html! {}).into_string();
        for asset in [
            "app.css",
            "vendor/htmx.min.js",
            "vendor/hx-sse.min.js",
            "vendor/alpine.min.js",
        ] {
            assert!(page.contains(&format!("/assets/{asset}?v=")), "{asset}");
        }
    }
}
