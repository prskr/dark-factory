//! The component library of the web UI (`docs/design-system.md`).
//!
//! Pages are composed from these maud functions. They take typed arguments (`ButtonKind`, `Tone`,
//! `TaskStatus`) and only use semantic color tokens from `styles/input.css`, never a raw palette
//! color. Call sites add layout (spacing, grid placement) around them, not styling.

pub mod actions;
pub mod badges;
pub mod forms;
pub mod icons;
pub mod lists;
pub mod overlay;
pub mod shell;
pub mod transcript;

pub use actions::{
    ButtonKind, ICON_BUTTON_CLASS, action_class, button, link_button, submit_button,
};
pub use badges::{Tone, chip, filter_chip, status_badge, tone_badge};
pub use forms::{
    FormErrors, MenuItem, field, field_error, form_error, search_menu, select_field, text_field,
    textarea_field,
};
pub use icons::{Icon, icon};
pub use lists::{definition_list, empty_state, scroll_box, section};
pub use overlay::{details_panel, dialog};
pub use shell::{avatar, bottom_bar, page_header, rail};

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
            "details_panel",
            "section",
            "definition_list",
            "empty_state",
            "scroll_box",
            "user_message",
            "agent_message",
            "thought",
            "pending_message",
            "tool_row",
            "code_block",
            "plan",
            "turn_end",
            "notice",
            "unreadable",
            "rail",
            "bottom_bar",
            "page_header",
            "avatar",
            "icon",
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
