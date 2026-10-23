//! Dialogs and panels: a native `<dialog>` that is a bottom sheet on phones, and the details
//! panel of the task workspace (bottom sheet, slide-over or column by width).

use maud::{Markup, html};

use super::actions::ICON_BUTTON_CLASS;
use super::icons::{Icon, icon};

/// A modal dialog opened as soon as it is inserted into the page (`showModal()` provides focus
/// trapping and Escape) and removed when it closes. Below 1024 px it is a bottom sheet.
pub fn dialog(id: &str, title: &str, body: Markup) -> Markup {
    let title_id = format!("{id}-title");
    html! {
        dialog id=(id) aria-labelledby=(title_id) x-data x-init="$el.showModal()" "x-on:close"="$el.remove()"
            class="fixed inset-x-0 bottom-0 m-0 mt-auto w-full max-w-none rounded-t-2xl border border-line bg-panel p-0 text-ink backdrop:bg-black/50 lg:inset-0 lg:m-auto lg:h-fit lg:max-w-xl lg:rounded-2xl" {
            div class="flex items-center justify-between gap-2 px-4 pt-3 lg:px-5" {
                h2 id=(title_id) class="text-base font-semibold" { (title) }
                form method="dialog" {
                    button class=(ICON_BUTTON_CLASS) aria-label="Close" { (icon(Icon::Close)) }
                }
            }
            div class="px-4 pb-4 lg:px-5" { (body) }
        }
    }
}

/// The details of a workspace. It expects the parent to hold the Alpine state `details`
/// (open or not); from 1280 px it is a permanent column and ignores it.
pub fn details_panel(title: &str, body: Markup) -> Markup {
    html! {
        aside id="ws-details" class="ws-details" aria-label=(title) ":data-open"="details" {
            div class="flex items-center justify-between px-4 pt-3 xl:hidden" {
                h2 class="text-base font-semibold" { (title) }
                button type="button" class=(ICON_BUTTON_CLASS) aria-label="Close details" "x-on:click"="details = false" { (icon(Icon::Close)) }
            }
            div class="space-y-5 p-4" { (body) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_is_native_labelled_and_closable() {
        let html = dialog("composer", "New task", html! { p { "x" } }).into_string();
        assert!(
            html.starts_with("<dialog id=\"composer\" aria-labelledby=\"composer-title\""),
            "{html}"
        );
        assert!(html.contains("x-init=\"$el.showModal()\""));
        assert!(html.contains("aria-label=\"Close\"") && html.contains("method=\"dialog\""));
        assert!(
            html.contains("fixed inset-x-0 bottom-0"),
            "bottom sheet on phones"
        );
    }

    #[test]
    fn details_panel_has_a_name_and_a_close_button() {
        let html = details_panel("Details", html! { "x" }).into_string();
        assert!(
            html.contains("aria-label=\"Details\"")
                && html.contains("aria-label=\"Close details\""),
            "{html}"
        );
        assert!(html.contains("size-11"));
    }
}
