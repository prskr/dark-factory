//! Form fields, validation errors and the menu with search.

use maud::{Markup, html};

use super::icons::{Icon, icon};

/// Validation errors by field name; the empty name is a message for the whole form.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FormErrors(Vec<(&'static str, String)>);

impl FormErrors {
    pub fn add(&mut self, field: &'static str, message: impl Into<String>) {
        self.0.push((field, message.into()));
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, field: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(f, _)| *f == field)
            .map(|(_, m)| m.as_str())
    }

    /// Turns the collected errors into a result: `Ok(value)` if there are none.
    pub fn into_result<T>(self, value: T) -> Result<T, Self> {
        if self.is_empty() {
            Ok(value)
        } else {
            Err(self)
        }
    }
}

/// Classes of text inputs, selects and textareas; 16 px text so phones do not zoom on focus.
pub const INPUT_CLASS: &str = "min-h-11 w-full rounded-lg border border-line-strong bg-panel px-3 py-2 text-base text-ink placeholder:text-faint lg:min-h-10";

/// A labelled control with an optional hint and the field's error.
pub fn field(
    label: &str,
    name: &str,
    errors: &FormErrors,
    hint: Option<&str>,
    control: Markup,
) -> Markup {
    html! {
        div class="space-y-1" {
            label for=(name) class="block text-sm font-medium text-muted" { (label) }
            (control)
            @if let Some(hint) = hint { p class="text-xs text-faint" { (hint) } }
            (field_error(errors.get(name)))
        }
    }
}

/// The error message of a field or an input group, if any.
pub fn field_error(error: Option<&str>) -> Markup {
    html! {
        @if let Some(error) = error { p class="text-sm text-danger" role="alert" { (error) } }
    }
}

/// A single-line input.
pub fn text_field(
    label: &str,
    name: &str,
    value: &str,
    errors: &FormErrors,
    hint: Option<&str>,
) -> Markup {
    field(
        label,
        name,
        errors,
        hint,
        html! { input id=(name) name=(name) type="text" value=(value) class=(INPUT_CLASS); },
    )
}

/// A multi-line input.
pub fn textarea_field(
    label: &str,
    name: &str,
    value: &str,
    rows: u8,
    errors: &FormErrors,
) -> Markup {
    field(
        label,
        name,
        errors,
        None,
        html! { textarea id=(name) name=(name) rows=(rows) class=(INPUT_CLASS) { (value) } },
    )
}

/// A select with an optional empty choice.
pub fn select_field(
    label: &str,
    name: &str,
    options: &[(String, String)],
    selected: &str,
    empty_label: Option<&str>,
    errors: &FormErrors,
) -> Markup {
    field(
        label,
        name,
        errors,
        None,
        html! {
            select id=(name) name=(name) class=(INPUT_CLASS) {
                @if let Some(empty) = empty_label { option value="" selected[selected.is_empty()] { (empty) } }
                @for (value, text) in options {
                    option value=(value) selected[value == selected] { (text) }
                }
            }
        },
    )
}

/// A form-wide error message, if any.
pub fn form_error(errors: &FormErrors) -> Markup {
    html! {
        @if let Some(error) = errors.get("") {
            p class="rounded-lg bg-danger-soft px-3 py-2 text-sm text-danger" role="alert" { (error) }
        }
    }
}

/// One entry of a [`search_menu`].
#[derive(Debug, Clone)]
pub struct MenuItem {
    pub href: String,
    pub label: String,
    /// Secondary text; also searched. For example a key or a count.
    pub hint: String,
    pub current: bool,
}

/// A button that opens a menu of links with a search field (Alpine). The search filters the
/// rendered entries by label and hint in the browser.
pub fn search_menu(button_label: &str, items: &[MenuItem]) -> Markup {
    html! {
        div class="relative" x-data="{ open: false, q: '' }" "x-on:keydown.escape"="open = false" "x-on:click.outside"="open = false" {
            button type="button" aria-haspopup="true" ":aria-expanded"="open" "x-on:click"="open = !open"
                class="inline-flex min-h-11 items-center gap-1 rounded-full border border-line bg-panel px-3 text-sm text-muted hover:bg-sunk lg:min-h-8" {
                (button_label) (icon(Icon::ChevronDown))
            }
            div x-show="open" x-cloak
                class="absolute left-0 z-20 mt-1 w-64 rounded-xl border border-line bg-panel p-2 shadow-lg" {
                label class="sr-only" for="menu-search" { "Search" }
                input id="menu-search" type="search" placeholder="Search" x-model="q" "x-effect"="open && $nextTick(() => $el.focus())"
                    class=(INPUT_CLASS);
                ul class="mt-2 max-h-64 overflow-y-auto" {
                    @for item in items {
                        li data-search=(format!("{} {}", item.label, item.hint).to_lowercase())
                            "x-show"="$el.dataset.search.includes(q.trim().toLowerCase())" {
                            a href=(item.href) aria-current=[item.current.then_some("true")]
                                class={"flex min-h-11 items-center justify-between gap-2 rounded-lg px-2 text-sm hover:bg-sunk lg:min-h-9 " (if item.current { "text-accent-text font-medium" } else { "text-ink" })} {
                                span class="truncate" { (item.label) }
                                span class="font-mono text-xs text-faint" { (item.hint) }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_shows_label_hint_and_error() {
        let mut errors = FormErrors::default();
        errors.add("name", "Name is required.");
        let html = text_field("Name", "name", "x<y", &errors, Some("Short.")).into_string();
        assert!(html.contains("<label for=\"name\""), "{html}");
        assert!(html.contains("value=\"x&lt;y\"") && html.contains("Short."));
        assert!(html.contains("role=\"alert\"") && html.contains("Name is required."));
        assert!(html.contains("min-h-11"));
    }

    #[test]
    fn select_marks_the_chosen_option() {
        let options = vec![
            ("a".to_string(), "A".to_string()),
            ("b".to_string(), "B".to_string()),
        ];
        let html =
            select_field("L", "n", &options, "b", None, &FormErrors::default()).into_string();
        assert!(html.contains("<option value=\"b\" selected>"), "{html}");
        assert!(!html.contains("<option value=\"a\" selected>"));
    }

    #[test]
    fn menu_lists_every_entry_with_search_data() {
        let items = vec![
            MenuItem {
                href: "/tasks?project=DF".into(),
                label: "Dark Factory".into(),
                hint: "DF".into(),
                current: true,
            },
            MenuItem {
                href: "/tasks?project=WEB".into(),
                label: "Website".into(),
                hint: "WEB".into(),
                current: false,
            },
        ];
        let html = search_menu("All projects", &items).into_string();
        assert!(
            html.contains("data-search=\"dark factory df\"")
                && html.contains("data-search=\"website web\""),
            "{html}"
        );
        assert!(html.contains("x-model=\"q\"") && html.contains("aria-current=\"true\""));
    }

    #[test]
    fn form_error_renders_only_the_form_wide_message() {
        let mut errors = FormErrors::default();
        errors.add("", "Broken.");
        assert!(form_error(&errors).into_string().contains("Broken."));
        assert!(form_error(&FormErrors::default()).into_string().is_empty());
    }
}
