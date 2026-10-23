//! Sections, definition lists and empty states.

use maud::{Markup, html};

/// A titled block of a details panel or page.
pub fn section(title: &str, body: Markup) -> Markup {
    html! {
        section class="space-y-2.5 border-b border-line px-5 py-4" {
            h2 class="text-xs font-medium uppercase tracking-wide text-faint" { (title) }
            (body)
        }
    }
}

/// Label and value pairs in a 76 px label column, for example project, persona and branch.
pub fn definition_list(items: &[(&str, Markup)]) -> Markup {
    html! {
        dl class="grid grid-cols-[76px_minmax(0,1fr)] gap-x-3 gap-y-2 text-[13px]" {
            @for (label, value) in items {
                dt class="text-faint" { (label) }
                dd class="min-w-0 break-words font-medium" { (value) }
            }
        }
    }
}

/// What a list shows when it has no entries.
pub fn empty_state(text: &str) -> Markup {
    html! { p class="px-4 py-8 text-center text-sm text-faint" { (text) } }
}

/// Where an empty state sits; the sizes follow the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptySize {
    /// A configuration list on a page.
    Page,
    /// The task list column; larger type and padding on phones.
    List,
    /// The transcript column without a selected task.
    Workspace,
}

/// What a list or column shows when there is nothing to show yet.
pub struct EmptyPanel<'a> {
    pub size: EmptySize,
    /// A round icon above the title (the workspace).
    pub icon: Option<Markup>,
    pub title: &'a str,
    /// One sentence on what the thing is for; may be empty.
    pub text: &'a str,
    /// An extra sentence shown on phones only.
    pub phone_hint: Option<&'a str>,
    /// The action: a button or a text link; may be empty.
    pub action: Markup,
}

impl EmptyPanel<'_> {
    pub fn render(self) -> Markup {
        let (container, heading, sentence) = match self.size {
            EmptySize::Page => (
                "flex flex-col items-center gap-3 px-6 py-10 text-center",
                "text-base font-semibold",
                "max-w-sm text-sm text-muted",
            ),
            EmptySize::List => (
                "flex flex-col items-center gap-2 px-6 py-12 text-center max-md:gap-2.5 max-md:py-16",
                "text-[15px] font-semibold max-md:text-[17px]",
                "text-sm text-muted",
            ),
            EmptySize::Workspace => (
                "flex max-w-sm flex-col items-center gap-2.5 px-6 text-center",
                "text-lg font-semibold",
                "text-muted",
            ),
        };
        html! {
            div class=(container) {
                @if let Some(icon) = self.icon {
                    span aria-hidden="true" class="flex size-14 items-center justify-center rounded-full bg-accent-soft text-accent-text" { (icon) }
                }
                h2 class=(heading) { (self.title) }
                @if !self.text.is_empty() || self.phone_hint.is_some() {
                    p class=(sentence) {
                        (self.text)
                        @if let Some(hint) = self.phone_hint { " " span class="md:hidden" { (hint) } }
                    }
                }
                (self.action)
            }
        }
    }
}

/// The empty state of a configuration list: a title, one sentence on what the item is for and its
/// create action (`action` may be empty).
pub fn empty_panel(title: &str, text: &str, action: Markup) -> Markup {
    EmptyPanel {
        size: EmptySize::Page,
        icon: None,
        title,
        text,
        phone_hint: None,
        action,
    }
    .render()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definition_list_pairs_terms_and_values() {
        let html = definition_list(&[("Project", html! { "Dark Factory" })]).into_string();
        // A 76 px label column, 13 px text, faint labels, left-aligned medium values.
        assert!(
            html.contains("grid grid-cols-[76px_minmax(0,1fr)] gap-x-3 gap-y-2 text-[13px]"),
            "{html}"
        );
        assert!(
            html.contains("<dt class=\"text-faint\">Project</dt>"),
            "{html}"
        );
        assert!(html.contains("<dd class=\"min-w-0 break-words font-medium\">Dark Factory</dd>"));
    }

    #[test]
    fn section_and_empty_states_render_their_content() {
        let runs = section("Runs", html! { "x" }).into_string();
        assert!(runs.contains(">Runs</h2>"));
        // Uppercase faint 12 px header, 16 / 20 px padding and a bottom border.
        assert!(runs.contains("border-b border-line px-5 py-4"), "{runs}");
        assert!(
            runs.contains("text-xs font-medium uppercase tracking-wide text-faint"),
            "{runs}"
        );
        assert!(empty_state("Nothing").into_string().contains("Nothing"));
        let panel =
            empty_panel("No tasks", "Create one.", html! { a href="/x" { "Go" } }).into_string();
        assert!(
            panel.contains(">No tasks</h2>")
                && panel.contains("Create one.")
                && panel.contains(">Go<")
        );
    }
}
