//! The frame around pages: navigation rail and bottom bar, profile menu, page header, avatar.

use maud::{Markup, html};

use super::icons::{Icon, icon, logo};
use crate::layout::{Nav, Shell};
use crate::requests::count_badge;

/// Where the docs of every version live (`docs/` of the repository, published per release).
const DOCS_URL: &str = "https://www.dark-factory.eu/docs";

const SECTIONS: [(Nav, &str, &str, Icon); 4] = [
    (Nav::Tasks, "/tasks", "Tasks", Icon::Tasks),
    (Nav::Requests, "/requests", "Requests", Icon::Inbox),
    (Nav::Projects, "/projects", "Projects", Icon::Projects),
    (
        Nav::EgressLog,
        "/egress/log",
        "Network traffic",
        Icon::Activity,
    ),
];

/// Where the rail's Settings link goes: the first section of [`SETTINGS_SECTIONS`].
const SETTINGS_HOME: &str = "/personas";

/// The sections of the settings area, in the order of the settings sidebar. The rail's Settings
/// link is current on each of them and opens the first ([`SETTINGS_HOME`]).
pub(crate) const SETTINGS_SECTIONS: [(Nav, &str, &str, Icon); 4] = [
    (Nav::Personas, SETTINGS_HOME, "Personas", Icon::Personas),
    (
        Nav::Credentials,
        "/credentials",
        "Credentials",
        Icon::Credentials,
    ),
    (
        Nav::ModelProviders,
        "/model-providers",
        "Model providers",
        Icon::Server,
    ),
    (
        Nav::Allowlist,
        "/egress/allowlist",
        "Egress rules",
        Icon::Shield,
    ),
];

/// Whether `nav` is a page of the settings area.
pub(crate) fn is_settings(nav: Nav) -> bool {
    SETTINGS_SECTIONS.iter().any(|(target, ..)| *target == nav)
}

/// A round badge with initials.
pub fn avatar(initials: &str) -> Markup {
    html! {
        span class="avatar inline-flex size-9 shrink-0 items-center justify-center rounded-full bg-accent-soft text-xs font-semibold text-accent-text" { (initials) }
    }
}

/// Title row of a page with the number of entries (if it lists any) and the page's actions on
/// the right.
pub fn page_header(title: &str, count: Option<usize>, actions: Markup) -> Markup {
    html! {
        header class="mb-5 flex items-center justify-between gap-4" {
            h1 class="text-2xl font-semibold" {
                (title)
                @if let Some(count) = count { " " span class="font-normal text-faint" { (count) } }
            }
            (actions)
        }
    }
}

/// The create action of the current screen: it creates what the screen lists. `href` is fetched
/// into the page's `#composer-slot` (a dialog).
#[derive(Debug, Clone, Copy)]
pub struct CreateAction<'a> {
    pub label: &'a str,
    pub href: &'a str,
}

/// Navigation rail from 768 px: logo, new task, the sections and the profile menu of
/// `shell`. Links show icons only; the section name is their accessible name and tooltip.
pub fn rail(active: Nav, compose_href: &str, shell: &Shell) -> Markup {
    html! {
        aside class="sticky top-0 z-30 hidden h-dvh w-[4.5rem] shrink-0 flex-col items-center gap-1 bg-rail py-3 md:flex" {
            a href="/tasks" aria-label="Dark Factory" title="Dark Factory"
                class="mb-2 flex size-11 items-center justify-center rounded-xl text-accent hover:bg-rail-hover" {
                (logo("size-7 shrink-0"))
            }
            (create_button(&CreateAction { label: "New task", href: compose_href }, "mb-3 inline-flex size-11 items-center justify-center rounded-xl bg-accent text-on-accent hover:brightness-95"))
            nav id="desktop-menu" aria-label="Main" class="flex flex-1 flex-col gap-1" {
                @for (target, href, label, glyph) in SECTIONS {
                    (rail_link(target == active, href, label, glyph, target == Nav::Requests))
                }
            }
            (rail_link(is_settings(active), SETTINGS_HOME, "Settings", Icon::Settings, false))
            (profile_menu(shell))
        }
    }
}

/// The settings sidebar from 768 px: the sections of the settings area with the open one marked.
pub fn settings_sidebar(active: Nav) -> Markup {
    html! {
        aside class="sticky top-0 hidden h-dvh w-56 shrink-0 flex-col gap-1 border-r border-line bg-panel px-3 py-8 md:flex" {
            h2 class="mb-3.5 px-3 text-lg font-semibold" { "Settings" }
            nav id="settings-menu" aria-label="Settings" class="flex flex-col gap-1" {
                @for (target, href, label, glyph) in SETTINGS_SECTIONS {
                    @let current = target == active;
                    a href=(href) aria-current=[current.then_some("page")]
                        class={"relative flex h-11 items-center gap-2.5 rounded-lg px-3 text-sm lg:h-9 [&_svg]:size-4 hover:bg-sunk " (if current { "bg-sunk font-medium text-ink before:absolute before:inset-y-2 before:left-0 before:w-[3px] before:rounded-full before:bg-accent" } else { "text-muted" })} {
                        (icon(glyph)) (label)
                    }
                }
            }
        }
    }
}

/// An icon-only link of the rail; the label is its accessible name and tooltip. `requests`
/// adds the pending count.
fn rail_link(current: bool, href: &str, label: &str, glyph: Icon, requests: bool) -> Markup {
    html! {
        a href=(href) aria-label=(label) title=(label) aria-current=[current.then_some("page")]
            class={"relative flex size-11 items-center justify-center rounded-lg hover:bg-rail-hover " (if current { "bg-rail-hover text-accent before:absolute before:inset-y-2 before:-left-3.5 before:w-1 before:rounded-r-full before:bg-accent" } else { "text-rail-ink" })} {
            (icon(glyph))
            @if requests { (count_badge("request-count-rail", 0)) }
        }
    }
}

/// Bottom bar below 768 px: Tasks, Requests and Projects at equal widths. Personas and
/// credentials are not part of it.
pub fn bottom_bar(active: Nav) -> Markup {
    html! {
        nav id="mobile-menu" aria-label="Main"
            class="bottombar fixed inset-x-0 bottom-0 z-30 flex h-16 items-stretch border-t border-line bg-panel md:hidden [body[data-view=task]_&]:hidden" {
            (bar_link(active, Nav::Tasks, "/tasks", "Tasks", Icon::Tasks))
            (bar_link(active, Nav::Requests, "/requests", "Requests", Icon::Inbox))
            (bar_link(active, Nav::Projects, "/projects", "Projects", Icon::Projects))
        }
    }
}

/// Round create button floating above the bottom bar at the right edge (phones only). Hidden on
/// the transcript screen like the bar.
pub fn floating_create(create: &CreateAction<'_>) -> Markup {
    create_button(
        create,
        "fixed bottom-20 right-4 z-30 flex h-14 w-14 items-center justify-center rounded-full bg-accent text-on-accent shadow-fab [&_svg]:size-7 [&_svg]:stroke-3 md:hidden [body[data-view=task]_&]:hidden",
    )
}

fn bar_link(active: Nav, target: Nav, href: &str, label: &str, glyph: Icon) -> Markup {
    let current = active == target;
    html! {
        a href=(href) aria-current=[current.then_some("page")]
            class={"flex min-h-11 flex-1 flex-col items-center justify-center gap-0.5 text-xs " (if current { "text-accent-text font-medium" } else { "text-muted" })} {
            span class="relative" {
                (icon(glyph))
                @if target == Nav::Requests { (count_badge("request-count-bar", 0)) }
            }
            (label)
        }
    }
}

/// The avatar of the signed-in board member with a menu: name, role, a link to the docs of the
/// running version, log out and the version.
/// The caller decides where it shows (phone header: `md:hidden`; rail). The menu opens below the
/// avatar on phones and above it, left-aligned, from 768 px.
pub fn profile_menu(shell: &Shell) -> Markup {
    let Shell {
        display_name,
        version,
    } = shell;
    html! {
        div class="relative" x-data="{ open: false }" "x-on:keydown.escape"="open = false" "x-on:click.outside"="open = false" {
            button type="button" aria-haspopup="true" ":aria-expanded"="open" "x-on:click"="open = !open"
                aria-label=(format!("Account menu of {display_name}"))
                class="inline-flex size-11 items-center justify-center rounded-full" {
                (avatar(&initials(display_name)))
            }
            div x-show="open" x-cloak class="absolute right-0 z-20 mt-1 w-56 rounded-xl border border-line bg-panel p-2 text-ink shadow-lg md:bottom-full md:left-0 md:right-auto md:mb-2 md:mt-0" {
                div class="px-2 py-2" {
                    div class="truncate text-sm font-medium" { (display_name) }
                    div class="text-xs text-faint" { "board member" }
                }
                div class="my-1 border-t border-line" {}
                a href=(format!("{DOCS_URL}/{version}/")) target="_blank" rel="noopener"
                    aria-label="Docs (opens in a new tab)"
                    class="flex min-h-11 w-full items-center gap-2 rounded-lg px-2 text-sm text-ink hover:bg-sunk" {
                    (icon(Icon::ExternalLink)) "Docs"
                }
                button type="button" hx-post="/logout"
                    class="flex min-h-11 w-full items-center gap-2 rounded-lg px-2 text-sm text-ink hover:bg-sunk" {
                    (icon(Icon::LogOut)) "Log out"
                }
                div class="my-1 border-t border-line" {}
                div class="px-3 py-2 text-xs text-faint" { "Dark Factory " (version) }
            }
        }
    }
}

/// The profile menu in the header of a phone page (hidden from 768 px, where the rail has it).
pub fn phone_profile_menu(shell: &Shell) -> Markup {
    html! { div class="md:hidden" { (profile_menu(shell)) } }
}

/// Up to two initials of a name, upper case; `?` for an empty name.
pub fn initials(name: &str) -> String {
    let letters: String = name
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();
    if letters.is_empty() {
        "?".to_string()
    } else {
        letters
    }
}

/// A button that fetches a create form into the page's `#composer-slot`.
fn create_button(create: &CreateAction<'_>, class: &str) -> Markup {
    html! {
        button type="button" aria-label=(create.label) title=(create.label) hx-get=(create.href) hx-target="#composer-slot" hx-swap="innerHTML" class=(class) {
            (icon(Icon::Plus))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEW_TASK: CreateAction<'static> = CreateAction {
        label: "New task",
        href: "/tasks/compose",
    };

    fn shell(display_name: &str, version: &str) -> Shell {
        Shell {
            display_name: display_name.into(),
            version: version.into(),
        }
    }

    #[test]
    fn rail_has_the_logo_first_and_icon_only_links_with_tooltips() {
        let html = rail(Nav::Tasks, "/tasks/compose", &shell("Peter Kurfer", "dev")).into_string();
        let logo_at = html.find("aria-label=\"Dark Factory\"").expect("logo");
        assert!(logo_at < html.find("aria-label=\"New task\"").unwrap());
        assert!(
            html.contains("href=\"/tasks\" aria-label=\"Dark Factory\""),
            "{html}"
        );
        assert!(html.contains("hx-get=\"/tasks/compose\""), "{html}");
        assert_eq!(html.matches("aria-current=\"page\"").count(), 1);
        for label in ["Tasks", "Projects", "Settings"] {
            assert!(
                html.contains(&format!("aria-label=\"{label}\" title=\"{label}\"")),
                "{label}"
            );
            // No visible text: the label never appears as element content.
            assert!(!html.contains(&format!(">{label}<")), "{label}");
            assert!(!html.contains(&format!("</svg>{label}")), "{label}");
        }
        assert!(html.contains("before:bg-accent"), "active bar at the edge");
        assert!(
            html.contains("hidden") && html.contains("md:flex"),
            "desktop only"
        );
    }

    #[test]
    fn rail_has_settings_above_the_avatar_and_no_settings_section_links() {
        let html = rail(Nav::Tasks, "/tasks/compose", &shell("Ada", "dev")).into_string();
        let settings = html
            .find("href=\"/personas\" aria-label=\"Settings\" title=\"Settings\"")
            .expect("Settings link");
        assert!(settings > html.find("Network traffic").unwrap());
        assert!(settings < html.find("Account menu of").unwrap());
        for hidden in [
            "/credentials",
            "/model-providers",
            "/egress/allowlist",
            "\"Personas\"",
            "\"Credentials\"",
            "\"Model providers\"",
            "\"Egress rules\"",
        ] {
            assert!(!html.contains(hidden), "{hidden}");
        }
    }

    #[test]
    fn rail_stacks_above_the_settings_sidebar_so_the_account_menu_stays_visible() {
        let html = rail(Nav::Personas, "/tasks/compose", &shell("Ada", "dev")).into_string();
        let aside = &html[html.find("<aside").unwrap()..];
        let aside = &aside[..aside.find('>').unwrap()];
        assert!(
            aside.contains("sticky") && aside.contains("z-30"),
            "{aside}"
        );
        let sidebar = settings_sidebar(Nav::Personas).into_string();
        assert!(!sidebar.contains("z-"), "{sidebar}");
    }

    #[test]
    fn settings_sidebar_links_are_touch_sized_below_1024_px() {
        let html = settings_sidebar(Nav::Personas).into_string();
        let links = html.matches("<a ").count();
        assert_eq!(links, 4);
        assert_eq!(html.matches("h-11 ").count(), links, "{html}");
        assert_eq!(html.matches("lg:h-9 ").count(), links, "{html}");
    }

    #[test]
    fn settings_is_current_on_every_settings_section() {
        for nav in [
            Nav::Personas,
            Nav::Credentials,
            Nav::ModelProviders,
            Nav::Allowlist,
        ] {
            let html = rail(nav, "/tasks/compose", &shell("Ada", "dev")).into_string();
            assert_eq!(html.matches("aria-current=\"page\"").count(), 1, "{nav:?}");
            assert!(html.contains("href=\"/personas\" aria-label=\"Settings\" title=\"Settings\" aria-current=\"page\""), "{nav:?}: {html}");
        }
    }

    #[test]
    fn rail_ends_with_the_avatar_trigger_and_has_no_logout_button_outside_the_menu() {
        let html = rail(Nav::Tasks, "/tasks/compose", &shell("Peter Kurfer", "dev")).into_string();
        assert!(
            html.contains("aria-label=\"Account menu of Peter Kurfer\""),
            "{html}"
        );
        assert!(html.contains(">PK<"));
        let menu_at = html.find("Account menu of").expect("trigger");
        assert!(
            !html[..menu_at].contains("hx-post=\"/logout\""),
            "log out only inside the menu"
        );
        assert_eq!(html.matches("hx-post=\"/logout\"").count(), 1);
        assert!(
            html.find("</nav>").unwrap() < menu_at,
            "the trigger is the last entry of the rail"
        );
    }

    #[test]
    fn bottom_bar_shows_tasks_requests_and_projects_without_a_create_action() {
        let html = bottom_bar(Nav::Tasks).into_string();
        assert!(html.contains("md:hidden") && html.contains("min-h-11"));
        for href in ["/tasks", "/requests", "/projects"] {
            assert!(html.contains(&format!("href=\"{href}\"")), "{href}");
        }
        assert_eq!(html.matches("flex-1 flex-col").count(), 3, "equal widths");
        for hidden in ["/personas", "/credentials", "Log out", "logout", "hx-get"] {
            assert!(!html.contains(hidden), "{hidden}");
        }
        assert_eq!(html.matches("aria-current=\"page\"").count(), 1);
    }

    #[test]
    fn bottom_bar_on_projects_marks_projects() {
        let html = bottom_bar(Nav::Projects).into_string();
        let current = &html[html.find("aria-current").unwrap()..];
        assert!(current.starts_with("aria-current=\"page\" class=\"flex min-h-11 flex-1 flex-col items-center justify-center gap-0.5 text-xs text-accent-text"), "{html}");
        assert!(current.contains("Projects"));
    }

    #[test]
    fn floating_create_is_a_round_phone_only_button_that_hides_on_the_transcript_screen() {
        let html = floating_create(&NEW_TASK).into_string();
        for class in [
            "fixed",
            "bottom-20",
            "right-4",
            "z-30",
            "h-14",
            "w-14",
            "rounded-full",
            "bg-accent",
            "md:hidden",
            "[body[data-view=task]_&amp;]:hidden",
            "shadow-fab",
            "[&amp;_svg]:size-7",
            "[&amp;_svg]:stroke-3",
        ] {
            assert!(html.contains(class), "{class}: {html}");
        }
        assert!(
            !html.contains("shadow-[") && !html.contains("rgb("),
            "{html}"
        );
        assert!(
            html.contains("aria-label=\"New task\"") && html.contains("hx-get=\"/tasks/compose\"")
        );
    }

    #[test]
    fn floating_create_shadow_and_plus_size_are_in_the_stylesheet() {
        let css = crate::app_css().unwrap();
        assert!(css.contains(".shadow-fab"), "shadow token utility");
        assert!(css.contains("--fab-shadow"), "shadow color token");
        assert!(
            css.contains(r".\[\&_svg\]\:size-7 svg{"),
            "28 px plus override rule"
        );
        assert!(
            css.contains(r".\[\&_svg\]\:stroke-3 svg{"),
            "stroke 3 plus override rule"
        );
    }

    #[test]
    fn profile_menu_holds_name_role_logout_and_version_and_has_an_accessible_name() {
        let html = profile_menu(&shell("Peter Kurfer", "2026.11.0")).into_string();
        assert!(html.contains(">PK<"), "{html}");
        assert!(html.contains("aria-label=\"Account menu of Peter Kurfer\""));
        assert!(html.contains("Peter Kurfer</div>") && html.contains("board member"));
        assert!(html.contains("hx-post=\"/logout\""));
        assert!(html.contains("Dark Factory 2026.11.0"), "{html}");
        assert!(html.contains("size-11") && html.contains("min-h-11"));
    }

    #[test]
    fn profile_menu_opens_the_docs_of_the_running_version_in_a_new_tab() {
        let html = profile_menu(&shell("Ada", "2026.11.0")).into_string();
        assert!(
            html.contains("href=\"https://www.dark-factory.eu/docs/2026.11.0/\""),
            "{html}"
        );
        assert!(html.contains("target=\"_blank\"") && html.contains("rel=\"noopener\""));
        assert!(html.contains("aria-label=\"Docs (opens in a new tab)\""));
        assert!(html.contains("min-h-11"), "touch target");
        let docs = html.find("Docs</a>").expect("Docs entry");
        assert!(
            docs < html.find("Log out").expect("Log out"),
            "Docs above Log out"
        );
    }

    #[test]
    fn profile_menu_docs_link_of_an_unstamped_build_points_to_the_dev_docs() {
        let html = profile_menu(&shell("Ada", "dev")).into_string();
        assert!(
            html.contains("href=\"https://www.dark-factory.eu/docs/dev/\""),
            "{html}"
        );
    }

    #[test]
    fn profile_menu_shows_dev_for_an_unstamped_build() {
        let html = profile_menu(&shell("Ada", "dev")).into_string();
        assert!(html.contains("Dark Factory dev"), "{html}");
    }

    #[test]
    fn phone_and_rail_render_the_same_menu() {
        let rail = rail(
            Nav::Tasks,
            "/tasks/compose",
            &shell("Ada Byron", "2026.11.0"),
        )
        .into_string();
        let menu = profile_menu(&shell("Ada Byron", "2026.11.0")).into_string();
        assert!(rail.contains(&menu), "{rail}");
    }

    #[test]
    fn initials_take_two_letters() {
        assert_eq!(initials("peter test"), "PT");
        assert_eq!(initials("Ada"), "A");
        assert_eq!(initials("Ada Byron King"), "AB");
        assert_eq!(initials("  "), "?");
    }

    #[test]
    fn page_header_and_avatar() {
        let html = page_header("Projects", Some(3), html! { "x" }).into_string();
        assert!(html.contains("<h1 class=\"text-2xl font-semibold\">Projects <span class=\"font-normal text-faint\">3</span></h1>"), "{html}");
        let plain = page_header("Tasks", None, html! {}).into_string();
        assert!(
            plain.contains(">Tasks</h1>") && !plain.contains("text-faint"),
            "{plain}"
        );
        assert!(avatar("DF").into_string().contains(">DF<"));
    }
}
