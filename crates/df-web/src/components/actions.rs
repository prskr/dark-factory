//! Buttons and links that look like buttons. Every target is at least 44 px high on screens
//! below 1024 px (`min-h-11`) and 36 px from there on.

use maud::{Markup, html};

/// Visual weight of an action: one primary action per view, the rest secondary or ghost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// Filled with the accent color; the one thing the view wants done.
    Primary,
    /// Outlined.
    Secondary,
    /// Text only, for toolbars and dismissals.
    Ghost,
}

/// Classes of an action; use it on elements that need their own attributes (`hx-*`, `x-*`).
pub fn action_class(kind: ButtonKind) -> &'static str {
    match kind {
        ButtonKind::Primary => {
            "inline-flex min-h-11 items-center justify-center gap-2 rounded-lg px-4 text-sm font-medium lg:min-h-9 bg-accent text-on-accent hover:brightness-95"
        }
        ButtonKind::Secondary => {
            "inline-flex min-h-11 items-center justify-center gap-2 rounded-lg px-4 text-sm font-medium lg:min-h-9 border border-line-strong bg-panel text-ink hover:bg-sunk"
        }
        ButtonKind::Ghost => {
            "inline-flex min-h-11 items-center justify-center gap-2 rounded-lg px-4 text-sm font-medium lg:min-h-9 text-muted hover:bg-sunk hover:text-ink"
        }
    }
}

/// Classes of a square icon-only control. It needs an `aria-label`.
pub const ICON_BUTTON_CLASS: &str = "inline-flex size-11 shrink-0 items-center justify-center rounded-lg text-muted hover:bg-sunk hover:text-ink lg:size-9";

/// Classes of the round accent action of a message box or card (Send, Create task): an icon in a
/// 44 px circle. It needs an `aria-label`.
pub const ROUND_ACTION_CLASS: &str = "inline-flex size-11 shrink-0 items-center justify-center rounded-full bg-accent text-on-accent hover:brightness-95";

/// The pill variant of [`ROUND_ACTION_CLASS`] for an action with a word (Save).
pub const ROUND_PILL_CLASS: &str = "inline-flex min-h-11 shrink-0 items-center justify-center gap-2 rounded-full bg-accent px-5 text-sm font-semibold text-on-accent hover:brightness-95";

/// A `type=button` button.
pub fn button(kind: ButtonKind, label: &str) -> Markup {
    html! { button type="button" class=(action_class(kind)) { (label) } }
}

/// A form's submit button.
pub fn submit_button(kind: ButtonKind, label: &str) -> Markup {
    html! { button type="submit" class=(action_class(kind)) { (label) } }
}

/// A link styled as a button.
pub fn link_button(kind: ButtonKind, href: &str, label: &str) -> Markup {
    html! { a href=(href) class=(action_class(kind)) { (label) } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_are_touch_sized_and_named() {
        for kind in [
            ButtonKind::Primary,
            ButtonKind::Secondary,
            ButtonKind::Ghost,
        ] {
            assert!(action_class(kind).contains("min-h-11"), "{kind:?}");
        }
        assert!(ICON_BUTTON_CLASS.contains("size-11"));
        let html = submit_button(ButtonKind::Primary, "Send").into_string();
        assert!(
            html.contains("type=\"submit\"") && html.contains(">Send<"),
            "{html}"
        );
        let html = link_button(ButtonKind::Secondary, "/x", "Open").into_string();
        assert!(html.contains("href=\"/x\"") && html.contains(">Open<"));
        assert!(
            button(ButtonKind::Ghost, "Cancel")
                .into_string()
                .contains("type=\"button\"")
        );
    }

    #[test]
    fn round_actions_are_touch_sized_accent_fills() {
        assert!(
            ROUND_ACTION_CLASS.contains("size-11") && ROUND_ACTION_CLASS.contains("rounded-full")
        );
        assert!(ROUND_PILL_CLASS.contains("min-h-11") && ROUND_PILL_CLASS.contains("rounded-full"));
        for class in [ROUND_ACTION_CLASS, ROUND_PILL_CLASS] {
            assert!(
                class.contains("bg-accent") && class.contains("text-on-accent"),
                "{class}"
            );
        }
    }

    #[test]
    fn primary_is_the_only_accent_fill() {
        assert!(action_class(ButtonKind::Primary).contains("bg-accent"));
        assert!(!action_class(ButtonKind::Secondary).contains("bg-accent"));
        assert!(!action_class(ButtonKind::Ghost).contains("bg-accent"));
    }
}
