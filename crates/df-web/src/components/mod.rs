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
pub use badges::{Tone, chip, filter_chip, harness_label, status_badge, tone_badge};
pub use forms::{
    FormErrors, MenuItem, field, field_error, form_error, readonly_field, search_menu,
    select_field, text_field, textarea_field,
};
pub use host_icon::{HostIconSize, host_icon};
pub use icons::{Icon, forge_icon, icon, logo};
pub use lists::{EmptyPanel, EmptySize, definition_list, empty_panel, empty_state, section};
pub use overlay::{DialogStyle, details_panel, dialog, dialog_close, dialog_with};
pub use project_host::{HostRole, ProjectHosts, issue_role, project_forge_kind, role_icon};
pub use shell::{CreateAction, avatar, bottom_bar, initials, page_header, profile_menu, rail};
pub use table::{Cell, data_table, td, th, tr, tr_with_id};
pub use transcript::QueuedState;

#[cfg(test)]
mod tests {
    const DOC: &str = include_str!("../../../../docs/design-system.md");
    const CSS: &str = include_str!("../../styles/input.css");

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
