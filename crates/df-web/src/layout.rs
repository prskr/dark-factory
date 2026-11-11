//! Shared page shell: head, navigation rail or bottom bar, and the vendored scripts.

use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use maud::{DOCTYPE, Markup, html};

use crate::WebState;
use crate::assets::{asset_url, get_asset};
use crate::auth::CurrentUser;
use crate::components::shell::is_settings;
use crate::components::{CreateAction, bottom_bar, floating_create, rail, settings_sidebar};

/// Alpine components in `scripts/`, compiled to `<name>.js` (`Alpine.data` on `alpine:init`),
/// loaded on every page before Alpine starts.
pub(crate) const COMPONENT_SCRIPTS: [&str; 7] = [
    "artifactSplit.js",
    "egress-log.js",
    "egress-rule.js",
    "markdown.js",
    "secret-field.js",
    "tabs.js",
    "workspace.js",
];

/// The diagram renderer, loaded by the `mermaidDiagram` component when a page has a diagram.
const MERMAID_ASSET: &str = "vendor/mermaid.min.js";

/// htmx extension in `scripts/sse-cleanup.ts`; loaded between htmx and `hx-sse`, so its cleanup
/// hook runs before the one of `hx-sse`.
pub(crate) const SSE_CLEANUP_SCRIPT: &str = "sse-cleanup.js";

/// htmx keeps the `open` state of `<details>` (tool calls, the runs list) when it morphs an
/// updated element. No settle step: after a delay it resets each swapped element that has the
/// `id` of an old one to the server's attributes, dropping what Alpine bound in the meantime
/// (`data-tab` of the details after switching tasks).
const HTMX_CONFIG: &str =
    r#"{"morphIgnore":["data-htmx-powered","open","style","aria-current"],"defaultSettleDelay":0}"#;

/// The navigation entry a page belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Tasks,
    Projects,
    /// The settings page of one project: a page of the settings area (sidebar marks Projects), and
    /// on phones a full page without the bottom bar and the create button.
    ProjectSettings,
    Personas,
    Credentials,
    ModelProviders,
    /// The settings page of one model provider: a page of the settings area (sidebar marks Model
    /// providers) without the phone navigation, like [`Nav::ProjectSettings`].
    ModelProviderSettings,
    /// Pending questions and approval requests.
    Requests,
    /// Egress rules (desktop only).
    Allowlist,
    /// Network traffic (desktop only).
    EgressLog,
    /// Pages outside the navigation (login errors, 403).
    None,
}

impl Nav {
    /// The entry of the settings sidebar this page belongs to: a project's settings page is part
    /// of Projects, a model provider's of Model providers, every other entry is its own.
    pub(crate) const fn settings_section(self) -> Self {
        match self {
            Self::ProjectSettings => Self::Projects,
            Self::ModelProviderSettings => Self::ModelProviders,
            other => other,
        }
    }
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

/// Who is signed in and which version runs: what the profile menu shows. Handlers of pages get it
/// as an extractor.
#[derive(Debug, Clone, Default)]
pub struct Shell {
    pub display_name: String,
    pub version: String,
    /// URL of the code theme stylesheet; none, no link.
    pub code_theme_url: Option<String>,
}

impl Shell {
    pub fn new(user: &CurrentUser, state: &WebState) -> Self {
        Self {
            display_name: user.display_name.clone(),
            version: state.version.clone(),
            code_theme_url: Some(state.code_theme.url().to_owned()),
        }
    }
}

impl FromRequestParts<WebState> for Shell {
    type Rejection = StatusCode;

    fn from_request_parts(
        parts: &mut Parts,
        state: &WebState,
    ) -> impl Future<Output = Result<Self, Self::Rejection>> + Send {
        std::future::ready(
            parts
                .extensions
                .get::<CurrentUser>()
                .map(|user| Self::new(user, state))
                .ok_or(StatusCode::UNAUTHORIZED),
        )
    }
}

/// Renders a full page of the usual width.
pub fn layout(title: &str, nav: Nav, shell: &Shell, body: Markup) -> Markup {
    document(title, nav, None, "/tasks/compose", shell, body)
}

/// Renders a full-width page of the task workspace. `compose_href` opens the composer with the
/// current project filter as default.
pub(crate) fn workspace_layout(
    title: &str,
    view: View,
    compose_href: &str,
    shell: &Shell,
    body: Markup,
) -> Markup {
    document(title, Nav::Tasks, Some(view), compose_href, shell, body)
}

fn document(
    title: &str,
    nav: Nav,
    view: Option<View>,
    compose_href: &str,
    shell: &Shell,
    body: Markup,
) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta name="color-scheme" content="light dark";
                meta name="htmx-config" content=(HTMX_CONFIG);
                title { (title) " - Dark Factory" }
                link rel="icon" type="image/svg+xml" href=(asset_url("logo.svg"));
                link rel="stylesheet" href=(asset_url("app.css"));
                @if let Some(url) = &shell.code_theme_url {
                    link rel="stylesheet" href=(url);
                }
                link rel="preload" as="font" type="font/woff2" crossorigin
                    href=(asset_url("fonts/inter-latin-wght-normal.woff2"));
                script src=(asset_url("vendor/htmx.min.js")) defer {}
                script src=(asset_url(SSE_CLEANUP_SCRIPT)) defer {}
                script src=(asset_url("vendor/hx-sse.min.js")) defer {}
                // Deferred scripts run in document order, so the components register first.
                @for script in COMPONENT_SCRIPTS {
                    script src=(asset_url(script)) defer {}
                }
                script src=(asset_url("vendor/alpine.min.js")) defer {}
            }
            // On a phone, browser Back must show the screen of the URL again: the row click set
            // `data-view` to `task`, and history navigation does not undo that.
            body class="bg-app font-sans text-ink antialiased" data-view=[view.map(View::as_str)]
                // Without the renderer (a `cargo` build without the npm packages) the component
                // leaves each diagram as source and says so.
                data-mermaid-src=[get_asset(MERMAID_ASSET).map(|_| asset_url(MERMAID_ASSET))]
                x-data=[view.map(|_| "")]
                "x-on:popstate.window"=[view.map(|_| "document.body.dataset.view = /^\\/tasks\\/[A-Z]/.test(location.pathname) ? 'task' : 'list'")] {
                div class="flex min-h-dvh" {
                    @if nav != Nav::None { (rail(nav, compose_href, shell)) }
                    @if is_settings(nav) { (settings_sidebar(nav)) }
                    main class={"min-w-0 flex-1 " (if view.is_some() { "" } else { "w-full px-4 py-4 pb-38 md:px-10 md:py-8 md:pb-8" })} {
                        (body)
                    }
                }
                @if nav != Nav::None {
                    @if !matches!(nav, Nav::ProjectSettings | Nav::ModelProviderSettings) {
                        (bottom_bar(nav))
                        (floating_create(&create_action(nav, compose_href)))
                    }
                    // Live pending count of the navigation (and the inbox list on its page).
                    div hx-sse:connect=(if nav == Nav::Requests { "/requests/events?list=1" } else { "/requests/events" }) hx-swap="none" {}
                }
                div id="composer-slot" {}
            }
        }
    }
}

/// What the floating create action does on this screen: a project on Projects, a task
/// everywhere else.
fn create_action(nav: Nav, compose_href: &str) -> CreateAction<'_> {
    match nav {
        Nav::Projects => CreateAction {
            label: "New project",
            href: "/projects/new",
        },
        _ => CreateAction {
            label: "New task",
            href: compose_href,
        },
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
    fn morph_keeps_the_open_state_of_details() {
        // The runs `<details>` (`runs_region`) stays open or closed across live updates only
        // because `open` is ignored when morphing.
        let config: serde_json::Value = serde_json::from_str(HTMX_CONFIG).unwrap();
        assert!(
            config["morphIgnore"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a == "open")
        );
    }

    #[test]
    fn swaps_keep_the_attributes_alpine_binds() {
        // With a settle delay, htmx resets every swapped element that has the `id` of an old one
        // to the server's attributes after Alpine bound its own, e.g. `data-tab` of the details.
        let config: serde_json::Value = serde_json::from_str(HTMX_CONFIG).unwrap();
        assert_eq!(config["defaultSettleDelay"], 0);
    }

    #[test]
    fn mobile_menu_lists_tasks_and_projects_but_no_other_configuration_pages() {
        let page =
            layout("Tasks", Nav::Tasks, &Shell::default(), html! { p { "x" } }).into_string();
        let mobile = nav_html(&page, "mobile-menu");
        assert!(mobile.contains("href=\"/tasks\"") && mobile.contains("href=\"/projects\""));
        assert!(mobile.contains("min-h-11"), "{mobile}");
        for hidden in ["/personas", "/credentials", "/egress", "logout"] {
            assert!(!mobile.contains(hidden), "{hidden} in mobile menu");
        }
        let desktop = nav_html(&page, "desktop-menu");
        for shown in ["/tasks", "/requests", "/egress/log"] {
            assert!(desktop.contains(shown), "{shown} missing on desktop");
        }
        for moved in [
            "/projects",
            "/credentials",
            "/model-providers",
            "/egress/allowlist",
        ] {
            assert!(!desktop.contains(moved), "{moved} left in the rail");
        }
        assert!(page.contains("aria-label=\"Settings\""), "{page}");
    }

    fn sidebar_html(nav: Nav) -> Option<String> {
        let page = layout("T", nav, &Shell::default(), html! {}).into_string();
        let start = page.find("id=\"settings-menu\"")?;
        let end = page[start..].find("</nav>").expect("sidebar end");
        Some(page[start..start + end].to_string())
    }

    #[test]
    fn settings_pages_show_the_sidebar_with_the_open_section_marked() {
        let cases = [
            (Nav::Projects, "/projects"),
            (Nav::Personas, "/personas"),
            (Nav::Credentials, "/credentials"),
            (Nav::ModelProviders, "/model-providers"),
            (Nav::Allowlist, "/egress/allowlist"),
        ];
        for (nav, current) in cases {
            let sidebar = sidebar_html(nav).expect("sidebar");
            assert!(sidebar.contains("Settings"), "{sidebar}");
            for (_, href) in cases {
                assert!(sidebar.contains(&format!("href=\"{href}\"")), "{href}");
            }
            for label in [
                "Projects",
                "Personas",
                "Credentials",
                "Model providers",
                "Egress rules",
            ] {
                assert!(
                    sidebar.contains(&format!(">{label}</a>"))
                        || sidebar.contains(&format!("</svg>{label}")),
                    "{label}"
                );
            }
            assert_eq!(sidebar.matches("<svg").count(), 5, "an icon per entry");
            assert_eq!(sidebar.matches("aria-current=\"page\"").count(), 1);
            let marked = &sidebar[sidebar.find("aria-current").unwrap()..];
            assert!(marked.contains("before:bg-accent"), "{sidebar}");
            let link = &sidebar[..sidebar.find("aria-current").unwrap()];
            assert!(
                link.ends_with(&format!("<a href=\"{current}\" ")),
                "{current}: {sidebar}"
            );
        }
    }

    #[test]
    fn projects_page_has_the_settings_sidebar_and_marks_settings_in_the_rail() {
        let page = layout("P", Nav::Projects, &Shell::default(), html! {}).into_string();
        let sidebar = sidebar_html(Nav::Projects).expect("sidebar");
        let first = &sidebar[sidebar.find("<a ").unwrap()..];
        assert!(
            first.starts_with("<a href=\"/projects\" aria-current=\"page\""),
            "{sidebar}"
        );
        assert!(
            page.contains("href=\"/projects\" aria-label=\"Settings\" title=\"Settings\" aria-current=\"page\""),
            "{page}"
        );
        let bar = nav_html(&page, "mobile-menu");
        for label in ["Tasks", "Requests", "Projects"] {
            assert!(bar.contains(label), "{label}");
        }
    }

    #[test]
    fn a_project_settings_page_has_the_sidebar_but_no_bottom_bar_and_no_create_button() {
        let page = layout("DF", Nav::ProjectSettings, &Shell::default(), html! {}).into_string();
        let sidebar = sidebar_html(Nav::ProjectSettings).expect("sidebar");
        let marked = &sidebar[..sidebar.find("aria-current").unwrap()];
        assert!(marked.ends_with("<a href=\"/projects\" "), "{sidebar}");
        assert!(
            page.contains("href=\"/projects\" aria-label=\"Settings\" title=\"Settings\" aria-current=\"page\""),
            "{page}"
        );
        assert!(!page.contains("id=\"mobile-menu\""), "{page}");
        assert!(!page.contains("fixed bottom-20"), "{page}");
        assert!(!page.contains("aria-label=\"New project\""), "{page}");
    }

    #[test]
    fn a_model_provider_settings_page_marks_model_providers_and_has_no_bottom_bar() {
        let page =
            layout("P", Nav::ModelProviderSettings, &Shell::default(), html! {}).into_string();
        let sidebar = sidebar_html(Nav::ModelProviderSettings).expect("sidebar");
        let marked = &sidebar[..sidebar.find("aria-current").unwrap()];
        assert!(
            marked.ends_with("<a href=\"/model-providers\" "),
            "{sidebar}"
        );
        assert!(!page.contains("id=\"mobile-menu\""), "{page}");
        assert!(!page.contains("fixed bottom-20"), "{page}");
    }

    #[test]
    fn the_sidebar_is_desktop_only_and_other_pages_have_none() {
        let page = layout("T", Nav::Personas, &Shell::default(), html! {}).into_string();
        let aside = &page[..page.find("id=\"settings-menu\"").unwrap()];
        let aside = &aside[aside.rfind("<aside").unwrap()..];
        assert!(
            aside.contains("hidden") && aside.contains("md:flex"),
            "{aside}"
        );
        for nav in [Nav::Tasks, Nav::Requests, Nav::EgressLog, Nav::None] {
            assert!(sidebar_html(nav).is_none(), "{nav:?}");
        }
    }

    #[test]
    fn phone_navigation_has_the_inbox_with_its_count_and_every_page_streams_it() {
        let page = layout("T", Nav::Tasks, &Shell::default(), html! {}).into_string();
        let bar = nav_html(&page, "mobile-menu");
        assert!(
            bar.contains("href=\"/requests\"") && bar.contains("id=\"request-count-bar\""),
            "{bar}"
        );
        let rail = nav_html(&page, "desktop-menu");
        assert!(
            rail.contains("href=\"/requests\"") && rail.contains("id=\"request-count-rail\""),
            "{rail}"
        );
        assert!(page.contains("hx-sse:connect=\"/requests/events\""));
        // The inbox page also streams its list.
        let inbox = layout("Requests", Nav::Requests, &Shell::default(), html! {}).into_string();
        assert!(inbox.contains("hx-sse:connect=\"/requests/events?list=1\""));
    }

    #[test]
    fn egress_entries_are_named_after_what_they_show() {
        let page = layout("T", Nav::Tasks, &Shell::default(), html! {}).into_string();
        let desktop = nav_html(&page, "desktop-menu");
        let link =
            "<a href=\"/egress/log\" aria-label=\"Network traffic\" title=\"Network traffic\"";
        assert!(desktop.contains(link), "{link} in {desktop}");
        let sidebar = sidebar_html(Nav::Allowlist).expect("sidebar");
        assert!(sidebar.contains("Egress rules"), "{sidebar}");
        assert!(!page.contains("Egress allowlist") && !page.contains("Egress log"));
    }

    #[test]
    fn rail_on_the_task_page_marks_tasks_current() {
        let page = layout("Tasks", Nav::Tasks, &Shell::default(), html! {}).into_string();
        let desktop = nav_html(&page, "desktop-menu");
        assert_eq!(
            desktop.matches("aria-current=\"page\"").count(),
            1,
            "{desktop}"
        );
        assert!(page.contains("aria-label=\"New task\"") && page.contains("hx-post=\"/logout\""));
    }

    #[test]
    fn the_page_declares_the_svg_favicon_and_the_rail_starts_with_the_logo() {
        let page = layout("T", Nav::Tasks, &Shell::default(), html! {}).into_string();
        assert!(
            page.contains("<link rel=\"icon\" type=\"image/svg+xml\" href=\"/assets/logo.svg?v="),
            "{page}"
        );
        let rail_at = page.find("<aside").unwrap();
        let first_link = &page[rail_at..];
        let first_link = &first_link[first_link.find("<a ").unwrap()..];
        assert!(
            first_link.starts_with("<a href=\"/tasks\" aria-label=\"Dark Factory\""),
            "{first_link}"
        );
    }

    #[test]
    fn create_action_follows_the_screen() {
        let projects = layout("P", Nav::Projects, &Shell::default(), html! {}).into_string();
        assert!(!nav_html(&projects, "mobile-menu").contains("hx-get"));
        let button = floating_button(&projects);
        assert!(
            button.contains("aria-label=\"New project\"")
                && button.contains("hx-get=\"/projects/new\"")
        );
        // The rail keeps the new-task action on every page.
        assert!(projects.contains("aria-label=\"New task\""));
        let tasks = layout("T", Nav::Tasks, &Shell::default(), html! {}).into_string();
        assert!(floating_button(&tasks).contains("aria-label=\"New task\""));
    }

    /// The floating create button: the `fixed` button after the bottom bar.
    fn floating_button(page: &str) -> &str {
        let bar = page.find("id=\"mobile-menu\"").unwrap();
        let rest = &page[bar + page[bar..].find("</nav>").unwrap()..];
        &rest[..rest.find("</button>").unwrap()]
    }

    #[test]
    fn main_pads_the_bottom_for_the_bar_and_the_floating_button_on_phones() {
        let page = layout("T", Nav::Tasks, &Shell::default(), html! {}).into_string();
        assert!(page.contains("pb-38 md:px-10 md:py-8 md:pb-8"), "{page}");
    }

    #[test]
    fn pages_outside_the_navigation_have_no_menu() {
        let page = layout("Forbidden", Nav::None, &Shell::default(), html! {}).into_string();
        assert!(!page.contains("mobile-menu") && !page.contains("desktop-menu"));
        assert!(page.contains("<title>Forbidden - Dark Factory</title>"));
    }

    #[test]
    #[cfg_attr(
        not(all(df_web_styles, df_web_vendor)),
        ignore = "needs compiled app.css and the third-party scripts: tailwindcss on PATH, pnpm install --frozen-lockfile in crates/df-web"
    )]
    fn page_loads_vendored_scripts_stylesheet_and_fonts_with_hashes() {
        let page = layout("T", Nav::Tasks, &Shell::default(), html! {}).into_string();
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
    #[cfg(df_web_vendor)]
    fn body_names_the_diagram_renderer_for_the_mermaid_component() {
        let page = layout("T", Nav::Tasks, &Shell::default(), html! {}).into_string();
        assert!(
            page.contains("data-mermaid-src=\"/assets/vendor/mermaid.min.js?v="),
            "{page}"
        );
        // The renderer itself is loaded by the component, not by every page.
        assert!(
            !page.contains("<script src=\"/assets/vendor/mermaid"),
            "{page}"
        );
    }

    #[test]
    #[cfg(not(df_web_vendor))]
    fn body_has_no_renderer_source_without_the_mermaid_asset() {
        let page = layout("T", Nav::Tasks, &Shell::default(), html! {}).into_string();
        assert!(!page.contains("data-mermaid-src"), "{page}");
        assert!(!page.contains("vendor/mermaid"), "{page}");
    }

    #[test]
    #[cfg_attr(
        not(all(df_web_scripts, df_web_vendor)),
        ignore = "needs compiled scripts and the third-party scripts: swc on PATH, pnpm install --frozen-lockfile in crates/df-web"
    )]
    fn sse_cleanup_extension_loads_between_htmx_and_hx_sse() {
        let page = layout("T", Nav::Tasks, &Shell::default(), html! {}).into_string();
        let htmx = page.find("/assets/vendor/htmx.min.js?v=").unwrap();
        let cleanup = page
            .find(&format!("/assets/{SSE_CLEANUP_SCRIPT}?v="))
            .expect(SSE_CLEANUP_SCRIPT);
        let sse = page.find("/assets/vendor/hx-sse.min.js?v=").unwrap();
        assert!(htmx < cleanup && cleanup < sse, "{page}");
    }

    #[test]
    fn sse_cleanup_extension_cancels_the_reader_before_hx_sse_aborts() {
        let js = crate::assets::script_source("sse-cleanup.ts");
        assert!(js.contains("htmx.registerExtension("), "{js}");
        assert!(js.contains("htmx_before_cleanup"), "{js}");
        // The rejection is handled and `hx-sse` finds no reader to cancel again.
        assert!(js.contains("reader.cancel().catch("), "{js}");
        assert!(js.contains("sse.reader = null"), "{js}");
    }

    #[test]
    #[cfg_attr(
        not(all(df_web_scripts, df_web_vendor)),
        ignore = "needs compiled scripts and the third-party scripts: swc on PATH, pnpm install --frozen-lockfile in crates/df-web"
    )]
    fn component_scripts_load_before_alpine_starts() {
        let page = layout("T", Nav::Tasks, &Shell::default(), html! {}).into_string();
        let alpine = page.find("/assets/vendor/alpine.min.js?v=").unwrap();
        for script in COMPONENT_SCRIPTS {
            let component = page.find(&format!("/assets/{script}?v=")).expect(script);
            assert!(component < alpine, "{script}");
        }
    }

    #[test]
    #[cfg_attr(
        not(df_web_styles),
        ignore = "needs compiled app.css: tailwindcss on PATH (bazel run //:bazel_env)"
    )]
    fn code_theme_stylesheet_is_linked_after_app_css_with_its_hash() {
        let shell = Shell {
            code_theme_url: Some(crate::markdown::CodeTheme::default().url().to_owned()),
            ..Shell::default()
        };
        let page = layout("T", Nav::Tasks, &shell, html! {}).into_string();
        let app = page.find("/assets/app.css?v=").expect("app.css");
        let theme = page.find("/assets/code-theme.css?v=").expect("code theme");
        assert!(app < theme, "{page}");
    }

    #[test]
    fn theme_comes_from_css_only() {
        let page = layout("T", Nav::Tasks, &Shell::default(), html! {}).into_string();
        assert!(page.contains("name=\"color-scheme\" content=\"light dark\""));
        // The only inline attributes are Alpine/htmx ones; no script chooses a theme.
        assert!(!page.contains("<script>") && !page.contains("matchMedia"));
    }

    #[test]
    fn workspace_marks_the_view_for_phones() {
        let page = workspace_layout(
            "T",
            View::Task,
            "/tasks/compose",
            &Shell::default(),
            html! {},
        )
        .into_string();
        assert!(page.contains("data-view=\"task\""));
        assert!(
            page.contains("x-on:popstate.window=\"document.body.dataset.view"),
            "{page}"
        );
        // Other pages carry no view: the body has only its classes (and the renderer URL).
        let plain = layout("T", Nav::Tasks, &Shell::default(), html! {}).into_string();
        assert!(
            plain.contains("<body class=\"bg-app font-sans text-ink antialiased\""),
            "{plain}"
        );
        assert!(!plain.contains("data-view=\"") && !plain.contains("x-on:popstate"));
    }

    #[test]
    fn task_list_on_a_phone_has_the_bottom_bar_and_no_rail_in_view() {
        let page = layout("T", Nav::Tasks, &Shell::default(), html! {}).into_string();
        let bar = nav_html(&page, "mobile-menu");
        assert!(bar.contains("md:hidden"), "bottom bar only below 768 px");
        for label in ["Tasks", "Requests", "Projects"] {
            assert!(bar.contains(label), "{label}");
        }
        assert!(!bar.contains("aria-label=\"New task\""));
        // The rail is hidden below 768 px.
        assert!(
            page.contains("class=\"sticky top-0 z-30 hidden h-dvh"),
            "{page}"
        );
    }

    #[test]
    #[cfg_attr(
        not(df_web_styles),
        ignore = "needs compiled app.css: tailwindcss on PATH (bazel run //:bazel_env)"
    )]
    fn the_bottom_bar_hides_on_the_transcript_screen_with_a_rule_that_beats_its_flex_utility() {
        let page = layout("T", Nav::Tasks, &Shell::default(), html! {}).into_string();
        let bar = nav_html(&page, "mobile-menu");
        // Same layer as `flex`, later in the sheet because it is a variant: it wins.
        assert!(
            bar.contains("flex") && bar.contains("[body[data-view=task]_&amp;]:hidden"),
            "{bar}"
        );
        let css = crate::app_css().unwrap();
        let rule = css.find("body[data-view=task]").map(|at| &css[at..]);
        let utility = r"\[body\[data-view\=task\]_\&\]\:hidden";
        let rule_at = css.find(utility).expect("variant rule in app.css");
        assert!(css[rule_at..].starts_with(utility), "{rule:?}");
        assert!(
            css[rule_at..][..200].contains("display:none"),
            "hidden by the rule"
        );
        // After the plain `.flex` utility in the stylesheet, so it overrides it.
        assert!(css.find(".flex{").unwrap() < rule_at);
    }

    #[test]
    fn pages_show_the_signed_in_user_and_the_version_in_the_profile_menu() {
        let shell = Shell {
            display_name: "Ada Byron".into(),
            version: "2026.11.0".into(),
            ..Shell::default()
        };
        let page = layout("Tasks", Nav::Tasks, &shell, html! {}).into_string();
        assert!(page.contains("Account menu of Ada Byron"), "{page}");
        assert!(page.contains("Dark Factory 2026.11.0"), "{page}");
    }

    #[test]
    fn composer_slot_exists_for_the_new_task_action() {
        let page = layout("T", Nav::Tasks, &Shell::default(), html! {}).into_string();
        assert!(page.contains("id=\"composer-slot\""));
    }
}
