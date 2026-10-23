//! Sections, definition lists, empty states and the scroll box for wide content.

use maud::{Markup, html};

/// A titled block of a details panel or page.
pub fn section(title: &str, body: Markup) -> Markup {
    html! {
        section class="space-y-2" {
            h2 class="text-xs font-semibold uppercase tracking-wide text-faint" { (title) }
            (body)
        }
    }
}

/// Label and value pairs, for example project, persona and branch.
pub fn definition_list(items: &[(&str, Markup)]) -> Markup {
    html! {
        dl class="space-y-2 text-sm" {
            @for (label, value) in items {
                div class="flex items-baseline justify-between gap-3" {
                    dt class="shrink-0 text-faint" { (label) }
                    dd class="min-w-0 break-words text-right text-ink" { (value) }
                }
            }
        }
    }
}

/// What a list shows when it has no entries.
pub fn empty_state(text: &str) -> Markup {
    html! { p class="px-4 py-8 text-center text-sm text-faint" { (text) } }
}

/// A box in which wide content (tables) scrolls instead of the page.
pub fn scroll_box(content: Markup) -> Markup {
    html! { div class="overflow-x-auto" { (content) } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definition_list_pairs_terms_and_values() {
        let html = definition_list(&[("Project", html! { "Dark Factory" })]).into_string();
        assert!(
            html.contains("<dt class=\"shrink-0 text-faint\">Project</dt>"),
            "{html}"
        );
        assert!(html.contains("Dark Factory</dd>"));
    }

    #[test]
    fn section_empty_state_and_scroll_box_render_their_content() {
        assert!(
            section("Runs", html! { "x" })
                .into_string()
                .contains(">Runs</h2>")
        );
        assert!(empty_state("Nothing").into_string().contains("Nothing"));
        assert!(
            scroll_box(html! { "t" })
                .into_string()
                .contains("overflow-x-auto")
        );
    }
}
