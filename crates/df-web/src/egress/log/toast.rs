//! What the allow and deny actions of the traffic page report: the content of a toast that links
//! to the rule (`components::toast_answer` shows it in the toast region), and the link itself.

use maud::{Markup, html};

use crate::components::{ToastTone, toast};

/// Link to the rules page with the rows of `target` (`host:port`, `host:any` for a deny rule)
/// highlighted.
pub(super) fn highlight_href(target: &str) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("highlight", target);
    format!("/egress/allowlist?{}", query.finish())
}

/// The toast of a created rule: `title`, what it means, and a link to the rule.
pub(super) fn created(title: &str, detail: Markup, rule_href: &str) -> Markup {
    toast(
        ToastTone::Success,
        html! {
            p class="font-semibold" { (title) }
            p class="mt-0.5 text-muted" { (detail) }
            a href=(rule_href) class="mt-1 inline-block text-accent-text hover:underline" { "View rule" }
        },
    )
}
