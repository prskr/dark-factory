//! The frame around pages: navigation rail (desktop), bottom bar (phone), page header, avatar.

use maud::{Markup, html};

use super::icons::{Icon, icon};
use crate::layout::Nav;

const SECTIONS: [(Nav, &str, &str, Icon); 4] = [
    (Nav::Tasks, "/tasks", "Tasks", Icon::Tasks),
    (Nav::Projects, "/projects", "Projects", Icon::Projects),
    (Nav::Personas, "/personas", "Personas", Icon::Personas),
    (
        Nav::Credentials,
        "/credentials",
        "Credentials",
        Icon::Credentials,
    ),
];

/// A round badge with initials.
pub fn avatar(initials: &str) -> Markup {
    html! {
        span class="inline-flex size-9 shrink-0 items-center justify-center rounded-full bg-accent-soft text-xs font-semibold text-accent-text" { (initials) }
    }
}

/// Title row of a page with the page's actions on the right.
pub fn page_header(title: &str, actions: Markup) -> Markup {
    html! {
        div class="mb-4 flex items-center justify-between gap-3" {
            h1 class="text-xl font-semibold" { (title) }
            (actions)
        }
    }
}

/// Navigation rail from 1024 px: new task, the sections and log out.
pub fn rail(active: Nav, compose_href: &str) -> Markup {
    html! {
        aside class="sticky top-0 hidden h-dvh w-[4.5rem] shrink-0 flex-col items-center gap-1 bg-rail py-3 lg:flex" {
            a href="/" aria-label="Dark Factory" class="mb-2 rounded-full" { (avatar("DF")) }
            (new_task_button(compose_href, "mb-3 inline-flex size-11 items-center justify-center rounded-xl bg-accent text-on-accent hover:brightness-95"))
            nav id="desktop-menu" aria-label="Main" class="flex flex-1 flex-col gap-1" {
                @for (target, href, label, glyph) in SECTIONS {
                    @let current = target == active;
                    a href=(href) aria-current=[current.then_some("page")]
                        class={"flex w-14 flex-col items-center gap-0.5 rounded-lg py-2 text-[11px] hover:bg-rail-hover " (if current { "bg-rail-hover text-accent" } else { "text-rail-ink" })} {
                        (icon(glyph)) (label)
                    }
                }
            }
            button type="button" hx-post="/logout" aria-label="Log out"
                class="flex w-14 flex-col items-center gap-0.5 rounded-lg py-2 text-[11px] text-rail-ink hover:bg-rail-hover" {
                (icon(Icon::LogOut)) "Log out"
            }
        }
    }
}

/// Bottom bar below 1024 px: Tasks, new task and log out. Configuration pages are not part of it.
pub fn bottom_bar(active: Nav, compose_href: &str) -> Markup {
    let current = active == Nav::Tasks;
    html! {
        nav id="mobile-menu" aria-label="Main"
            class="bottombar fixed inset-x-0 bottom-0 z-30 flex h-16 items-stretch justify-around border-t border-line bg-panel lg:hidden" {
            a href="/tasks" aria-current=[current.then_some("page")]
                class={"flex min-h-11 flex-1 flex-col items-center justify-center gap-0.5 text-xs " (if current { "text-accent-text font-medium" } else { "text-muted" })} {
                (icon(Icon::Tasks)) "Tasks"
            }
            div class="flex flex-1 items-center justify-center" {
                (new_task_button(compose_href, "inline-flex size-12 items-center justify-center rounded-full bg-accent text-on-accent"))
            }
            button type="button" hx-post="/logout" class="flex min-h-11 flex-1 flex-col items-center justify-center gap-0.5 text-xs text-muted" {
                (icon(Icon::LogOut)) "Log out"
            }
        }
    }
}

/// The new-task action. `compose_href` opens the composer; the response lands in the page's
/// `#composer-slot`.
fn new_task_button(compose_href: &str, class: &str) -> Markup {
    html! {
        button type="button" aria-label="New task" hx-get=(compose_href) hx-target="#composer-slot" hx-swap="innerHTML" class=(class) {
            (icon(Icon::Plus))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rail_marks_the_current_section_and_names_icon_buttons() {
        let html = rail(Nav::Tasks, "/tasks/compose").into_string();
        assert!(
            html.contains("aria-label=\"New task\"") && html.contains("hx-get=\"/tasks/compose\""),
            "{html}"
        );
        assert_eq!(html.matches("aria-current=\"page\"").count(), 1);
        for label in ["Tasks", "Projects", "Personas", "Credentials", "Log out"] {
            assert!(
                html.contains(&format!(">{label}<")) || html.contains(&format!("</svg>{label}")),
                "{label}"
            );
        }
        assert!(
            html.contains("hidden") && html.contains("lg:flex"),
            "desktop only"
        );
    }

    #[test]
    fn bottom_bar_has_three_touch_targets_and_no_configuration() {
        let html = bottom_bar(Nav::Projects, "/tasks/compose").into_string();
        assert!(
            html.contains("lg:hidden") && html.contains("size-12") && html.contains("min-h-11")
        );
        assert!(html.contains("aria-label=\"New task\""));
        assert!(
            !html.contains("aria-current"),
            "projects is not in the bottom bar"
        );
        for hidden in ["/projects", "/personas", "/credentials"] {
            assert!(!html.contains(hidden), "{hidden}");
        }
    }

    #[test]
    fn page_header_and_avatar() {
        let html = page_header("Tasks", html! { "x" }).into_string();
        assert!(html.contains("<h1 class=\"text-xl font-semibold\">Tasks</h1>"));
        assert!(avatar("DF").into_string().contains(">DF<"));
    }
}
