//! The component library of the web UI (`docs/design-system.md`).
//!
//! Pages are composed from these maud functions. They take typed arguments (`ButtonKind`, `Tone`,
//! `TaskStatus`) and only use semantic color tokens from `styles/input.css`, never a raw palette
//! color. Call sites add layout (spacing, grid placement) around them, not styling.

pub mod actions;
pub mod badges;
pub mod forms;
pub mod host_icon;
pub mod icons;
pub mod lists;
pub mod overlay;
pub mod project_host;
pub mod shell;
pub mod table;
pub mod transcript;

pub use actions::{
    ButtonKind, ICON_BUTTON_CLASS, ROUND_ACTION_CLASS, ROUND_PILL_CLASS, action_class, button,
    link_button, submit_button,
};
pub use badges::{
    Tone, attached_badge, chip, filter_chip, harness_label, status_badge, tone_badge,
};
pub use forms::{
    FormErrors, MenuItem, field, field_error, form_error, form_group, icon_menu,
    mono_text_field_of_form, readonly_field, readonly_field_compact, search_menu, select_field,
    text_field, textarea_field,
};
pub use host_icon::{HostIconSize, host_icon};
pub use icons::{Icon, forge_icon, icon, logo};
pub use lists::{EmptyPanel, EmptySize, definition_list, empty_panel, empty_state, section};
pub use overlay::{
    DialogStyle, INITIAL_DETAILS_TAB, details_panel, details_panel_with_heading, dialog,
    dialog_close, dialog_with,
};
pub use project_host::{HostRole, ProjectHosts, issue_role, project_forge_kind, role_icon};
pub use shell::{
    CreateAction, avatar, bottom_bar, floating_create, initials, page_header,
    page_header_with_subtitle, phone_profile_menu, profile_menu, rail, settings_sidebar,
};
pub use table::{
    Cell, actions_cell, data_table, entry_button, entry_link, entry_tr, faint_separator, name_cell,
    separator, td, th, tr, tr_with_id,
};
pub use transcript::QueuedState;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    const DOC: &str = include_str!("../../../../docs/design-system.md");
    const CSS: &str = include_str!("../../styles/input.css");
    /// WCAG 2.x minimum for body text. No token pair is reserved for large text (3:1).
    const MIN_TEXT_CONTRAST: f64 = 4.5;
    /// Foreground and background tokens of the text pairs in `docs/design-system.md`,
    /// "Contrast ratios".
    const TEXT_PAIRS: [(&str, &str); 30] = [
        ("ink", "app"),
        ("ink", "panel"),
        ("ink", "raised"),
        ("ink", "bubble-me"),
        ("muted", "app"),
        ("muted", "panel"),
        ("muted", "sunk"),
        ("muted", "raised"),
        ("faint", "app"),
        ("faint", "panel"),
        ("faint", "sunk"),
        ("faint", "raised"),
        ("faint", "accent-soft"),
        ("danger", "raised"),
        ("danger", "app"),
        ("danger", "danger-soft"),
        ("accent-text", "panel"),
        ("accent-text", "accent-soft"),
        ("on-accent", "accent"),
        ("on-bubble-agent", "bubble-agent"),
        ("on-bubble-agent", "code-on-agent"),
        ("rail-ink", "rail"),
        ("accent", "rail"),
        ("status-running-fg", "status-running-bg"),
        ("status-waiting-fg", "status-waiting-bg"),
        ("status-queued-fg", "status-queued-bg"),
        ("status-completed-fg", "status-completed-bg"),
        ("status-failed-fg", "status-failed-bg"),
        ("status-limited-fg", "status-limited-bg"),
        ("status-review-fg", "status-review-bg"),
    ];

    /// Names of the variables in the light `:root` block of the stylesheet.
    fn tokens() -> Vec<String> {
        let block = CSS
            .split(":root {")
            .nth(1)
            .and_then(|b| b.split('}').next());
        block
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.trim().strip_prefix("--"))
            .filter_map(|line| line.split(':').next())
            .map(str::to_string)
            .collect()
    }

    /// The `--name: #rrggbb` colors of the first `:root { … }` block in `css`.
    fn colors(css: &str) -> HashMap<String, String> {
        let block = css
            .split(":root {")
            .nth(1)
            .and_then(|b| b.split('}').next());
        block
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.trim().strip_prefix("--")?.split_once(':'))
            .map(|(name, value)| {
                let value = value.trim().trim_end_matches(';');
                (name.to_string(), value.to_string())
            })
            .filter(|(_, value)| value.len() == 7 && value.starts_with('#'))
            .collect()
    }

    /// Light and dark colors; the dark theme keeps every light value it does not redefine.
    fn themes() -> [(&'static str, HashMap<String, String>); 2] {
        let light = colors(CSS);
        let mut dark = light.clone();
        let dark_css = CSS
            .split("@media (prefers-color-scheme: dark)")
            .nth(1)
            .expect("dark theme block in styles/input.css");
        dark.extend(colors(dark_css));
        [("light", light), ("dark", dark)]
    }

    /// Relative luminance of `#rrggbb` (WCAG 2.x).
    fn luminance(hex: &str) -> f64 {
        let channel = |at: usize| {
            let c = f64::from(u8::from_str_radix(&hex[at..at + 2], 16).unwrap()) / 255.0;
            if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5)
    }

    fn contrast(a: &str, b: &str) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn contrast_follows_the_wcag_formula() {
        assert!((contrast("#000000", "#ffffff") - 21.0).abs() < 1e-9);
        assert!((contrast("#ffffff", "#ffffff") - 1.0).abs() < 1e-9);
        // The lightest grey that meets 4.5:1 on white, and the next lighter one.
        assert!(contrast("#767676", "#ffffff") >= MIN_TEXT_CONTRAST);
        assert!(contrast("#777777", "#ffffff") < MIN_TEXT_CONTRAST);
    }

    #[test]
    fn every_text_pair_meets_the_minimum_contrast_in_both_themes() {
        for (theme, colors) in themes() {
            for (fg, bg) in TEXT_PAIRS {
                let (Some(fg_hex), Some(bg_hex)) = (colors.get(fg), colors.get(bg)) else {
                    panic!("`{fg}` or `{bg}` is not a hex color of the {theme} theme");
                };
                let ratio = contrast(fg_hex, bg_hex);
                assert!(
                    ratio >= MIN_TEXT_CONTRAST,
                    "{theme}: `{fg}` on `{bg}` is {ratio:.2}:1, below {MIN_TEXT_CONTRAST}:1"
                );
            }
        }
    }

    #[test]
    fn every_token_of_the_stylesheet_is_documented() {
        let tokens = tokens();
        assert!(tokens.len() > 20, "{tokens:?}");
        for token in tokens {
            // A `-fg` token shares its table row with the `-bg` token.
            let documented = token
                .strip_suffix("-fg")
                .map_or(token.clone(), |base| format!("{base}-bg"));
            assert!(
                DOC.contains(&format!("`{documented}`")),
                "token `{token}` is not in docs/design-system.md"
            );
        }
    }

    #[test]
    fn every_component_is_documented() {
        let components = [
            "ButtonKind",
            "button",
            "submit_button",
            "link_button",
            "action_class",
            "ICON_BUTTON_CLASS",
            "Tone",
            "status_badge",
            "tone_badge",
            "chip",
            "filter_chip",
            "FormErrors",
            "field",
            "text_field",
            "textarea_field",
            "select_field",
            "form_error",
            "search_menu",
            "MenuItem",
            "dialog",
            "dialog_with",
            "dialog_close",
            "details_panel",
            "section",
            "definition_list",
            "empty_state",
            "empty_panel",
            "user_message",
            "agent_message",
            "thought",
            "queued_message",
            "run_divider",
            "tool_row",
            "code_block",
            "plan",
            "notice",
            "unreadable",
            "rail",
            "bottom_bar",
            "CreateAction",
            "data_table",
            "ROUND_ACTION_CLASS",
            "QueuedState",
            "profile_menu",
            "initials",
            "logo",
            "page_header",
            "avatar",
            "icon",
            "forge_icon",
            "host_icon",
        ];
        for name in components {
            assert!(
                DOC.contains(&format!("`{name}")),
                "component `{name}` is not in docs/design-system.md"
            );
        }
        assert!(DOC.contains("## Review checklist"));
    }
}
