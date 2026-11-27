//! The look of an underline tab (the `PenPot` component `Tab`, style Underline): the selected tab is
//! medium weight with a 2 px accent line, the others are muted and regular. The behavior (roles,
//! keys, selection) is the caller's: the `tabs` component for page and details tabs, the
//! `markdownField` component for the tabs of a text answer.

use maud::{Markup, html};

/// Classes every underline tab shares. The caller adds its height, padding and text size. The tab
/// also needs `aria-selected`, which the classes below react to, and holds a [`tab_marker`].
pub const TAB_CLASS: &str =
    "group relative min-h-11 text-muted aria-selected:font-medium aria-selected:text-ink";

/// How far the accent line is inset from the sides of its tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkerInset {
    /// The line spans the tab (tabs with little padding).
    None,
    /// 8 px from each side (tabs with padding of their own).
    Narrow,
}

/// The 2 px accent line under the selected tab, shown only while the tab has
/// `aria-selected="true"`. Put it first inside the tab, which carries [`TAB_CLASS`].
pub fn tab_marker(inset: MarkerInset) -> Markup {
    // Full class strings: the stylesheet is built from the literals in the source.
    let class = match inset {
        MarkerInset::None => {
            "absolute inset-x-0 bottom-0 hidden h-0.5 rounded-full bg-accent-text group-aria-selected:block"
        }
        MarkerInset::Narrow => {
            "absolute inset-x-2 bottom-0 hidden h-0.5 rounded-full bg-accent-text group-aria-selected:block"
        }
    };
    html! { span aria-hidden="true" class=(class) {} }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marker_is_shown_only_for_the_selected_tab_and_hidden_from_screen_readers() {
        for inset in [MarkerInset::None, MarkerInset::Narrow] {
            let html = tab_marker(inset).into_string();

            assert!(html.contains("aria-hidden=\"true\""), "{html}");
            assert!(
                html.contains(" hidden ") && html.contains("group-aria-selected:block"),
                "{html}"
            );
            assert!(
                html.contains("h-0.5") && html.contains("bg-accent-text"),
                "{html}"
            );
        }
    }

    #[test]
    fn a_tab_is_medium_weight_and_ink_only_while_selected() {
        assert!(TAB_CLASS.contains("aria-selected:font-medium"));
        assert!(TAB_CLASS.contains("aria-selected:text-ink"));
        assert!(TAB_CLASS.contains("text-muted"));
    }
}
