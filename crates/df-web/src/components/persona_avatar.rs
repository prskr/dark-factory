//! The generated persona avatar (`specs/persona-identity/spec.md`, "Generated avatar"): the
//! initials of the display name on one of eight colors chosen from the persona's id. No stored
//! image, no request to another host. Colors are the `avatar-<n>` classes of `styles/input.css`.

use maud::{Markup, html};
use uuid::Uuid;

use super::shell::initials;

/// Number of colors in the palette (`--avatar-1-bg` to `--avatar-8-bg`).
const PALETTE_SIZE: u32 = 8;

/// The avatar sizes of the design (`Avatar · palette and sizes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvatarSize {
    /// 56 px, the persona form; 48 px on a phone.
    Form,
    /// 40 px, the persona card.
    Card,
    /// 32 px, the transcript.
    Transcript,
    /// 20 px, task rows and the task details.
    Row,
}

impl AvatarSize {
    const fn class(self) -> &'static str {
        match self {
            Self::Form => "size-12 text-base md:size-14 md:text-lg",
            Self::Card => "size-10 text-sm",
            Self::Transcript => "size-8 text-xs",
            Self::Row => "size-5 text-[9px]",
        }
    }
}

/// How a persona appears as an author: its avatar (by id) and the name with its `@handle`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersonaLabel {
    /// `None`: a neutral avatar.
    pub id: Option<Uuid>,
    pub display_name: String,
    /// Empty: only the display name is shown.
    pub handle: String,
}

impl From<&str> for PersonaLabel {
    /// A label with only a display name, for places that do not know the persona.
    fn from(display_name: &str) -> Self {
        Self {
            display_name: display_name.to_owned(),
            ..Self::default()
        }
    }
}

/// The color class of the persona: FNV-1a over the bytes of its id (`DefaultHasher` is not stable
/// across Rust releases, and the color must not change).
pub fn color_class(id: Uuid) -> String {
    let hash = id.as_bytes().iter().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
    });
    format!("avatar-{}", hash % PALETTE_SIZE + 1)
}

/// The avatar of persona `id` with `display_name`; `None` (a persona not yet saved) is neutral.
pub fn persona_avatar(id: Option<Uuid>, display_name: &str, size: AvatarSize) -> Markup {
    avatar_span(id, display_name, size, false)
}

/// [`persona_avatar`] for the persona form: inside the Alpine component `personaAvatar`
/// (`scripts/persona-avatar.ts`), the initials and the label follow the typed display name.
pub fn persona_avatar_preview(id: Option<Uuid>, display_name: &str, size: AvatarSize) -> Markup {
    avatar_span(id, display_name, size, true)
}

fn avatar_span(id: Option<Uuid>, display_name: &str, size: AvatarSize, preview: bool) -> Markup {
    let color = id.map_or_else(|| "bg-sunk text-muted".to_owned(), color_class);
    html! {
        span role="img" aria-label=(display_name)
            "x-bind:aria-label"=[preview.then_some("name")]
            "x-text"=[preview.then_some("initials")]
            class={"avatar persona-avatar no-underline hover:no-underline inline-flex shrink-0 items-center justify-center rounded-full font-semibold " (size.class()) " " (color)} {
            (initials(display_name))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: Uuid = Uuid::from_u128(0x0192_a000_0000_7000_8000_0000_0000_0001);

    #[test]
    fn an_avatar_inside_a_link_has_no_underline_in_any_state() {
        let html = persona_avatar(Some(ID), "Code Reviewer", AvatarSize::Row).into_string();

        assert!(html.contains("no-underline hover:no-underline"), "{html}");
    }

    #[test]
    fn initials_come_from_up_to_two_words_and_the_label_is_the_display_name() {
        let html = persona_avatar(Some(ID), "Code Reviewer", AvatarSize::Card).into_string();

        assert!(html.contains(">CR<"), "{html}");
        assert!(
            html.contains("role=\"img\"") && html.contains("aria-label=\"Code Reviewer\""),
            "{html}"
        );
        let three = persona_avatar(Some(ID), "Ada Byron King", AvatarSize::Card).into_string();
        assert!(three.contains(">AB<"), "{three}");
    }

    #[test]
    fn the_color_is_stable_for_an_id_and_independent_of_the_name() {
        let before = persona_avatar(Some(ID), "Code Reviewer", AvatarSize::Row).into_string();
        let renamed = persona_avatar(Some(ID), "Strict Reviewer", AvatarSize::Row).into_string();

        let color = |html: &str| {
            html.split("avatar-")
                .nth(1)
                .map(|rest| rest[..1].to_owned())
        };
        assert_eq!(color(&before), color(&renamed));
        // FNV-1a of the id bytes: this value must never change, or every avatar changes color.
        assert_eq!(color_class(ID), "avatar-6");
    }

    #[test]
    fn every_color_of_the_palette_is_used() {
        let used: std::collections::BTreeSet<_> = (0..200_u128)
            .map(|n| color_class(Uuid::from_u128(n.wrapping_mul(0x9e37_79b9_7f4a_7c15))))
            .collect();

        assert_eq!(used.len(), PALETTE_SIZE as usize, "{used:?}");
    }

    #[test]
    fn a_persona_that_is_not_saved_has_the_neutral_color() {
        let html = persona_avatar(None, "Code Reviewer", AvatarSize::Form).into_string();

        assert!(
            html.contains("bg-sunk text-muted") && !html.contains("avatar-"),
            "{html}"
        );
    }

    #[test]
    fn the_markup_makes_no_request_to_another_host() {
        let html = persona_avatar(Some(ID), "Code Reviewer", AvatarSize::Card).into_string();

        for url in ["http:", "https:", "src=", "url(", "//"] {
            assert!(!html.contains(url), "{url}: {html}");
        }
    }
}
