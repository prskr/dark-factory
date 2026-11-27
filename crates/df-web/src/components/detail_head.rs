//! The head of the detail of a task or chat: the focus target after a row of the list was
//! selected (`scripts/focusAfterSwap.ts`), named by a screen-reader-only heading.

use maud::{Markup, html};

use crate::htmx::Region;

/// The head element `region` around `content`. It takes focus programmatically (`tabindex="-1"`)
/// and shows the focus ring only after a keyboard action, drawn inset because the head spans the
/// column. Its first child is the `h2` that names it: `key` and `title`. The list column owns the
/// page's `h1`, and the visible titles cannot carry the name (rename button, phone-only chat
/// title), so the heading is for screen readers only.
pub(crate) fn detail_head(region: Region, key: &str, title: &str, content: Markup) -> Markup {
    html! {
        header id=(region.id()) tabindex="-1" aria-labelledby="detail-title"
            class="border-b border-line bg-panel px-2 py-2 focus-visible:-outline-offset-2 lg:px-4" {
            h2 id="detail-title" class="sr-only" { (key) " " (title) }
            (content)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_head_is_a_focus_target_named_by_a_screen_reader_only_heading() {
        let html = detail_head(
            Region::TaskHead,
            "DF-21",
            "Fix login",
            html! { p { "row" } },
        )
        .into_string();

        let tag = &html[..html.find('>').unwrap()];
        assert!(
            tag.contains("id=\"task-head\"")
                && tag.contains("tabindex=\"-1\"")
                && tag.contains("aria-labelledby=\"detail-title\"")
                && tag.contains("focus-visible:-outline-offset-2"),
            "{tag}"
        );
        let content = &html[html.find('>').unwrap() + 1..];
        assert!(
            content.starts_with(
                "<h2 id=\"detail-title\" class=\"sr-only\">DF-21 Fix login</h2><p>row</p>"
            ),
            "{html}"
        );
    }
}
