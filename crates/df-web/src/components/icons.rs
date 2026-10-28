//! Icons: a subset of Lucide (ISC licence, `LICENSE-Lucide.txt`), inlined as SVG.

use maud::{Markup, PreEscaped, html};

/// The available icons. Add one by copying the inner elements of its Lucide SVG into [`Icon::body`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Plus,
    Tasks,
    Projects,
    Personas,
    Credentials,
    /// Egress allowlist (Lucide `shield-check`).
    Shield,
    /// Egress log (Lucide `activity`).
    Activity,
    /// The requests inbox (Lucide `inbox`).
    Inbox,
    LogOut,
    PanelRight,
    Close,
    Search,
    Check,
    ChevronDown,
    ChevronLeft,
    ChevronRight,
    Pencil,
    PullRequest,
    Issue,
    Send,
    ArrowUp,
    Link,
    /// Delete (Lucide `trash`).
    Trash,
    /// Row actions menu (Lucide `ellipsis`).
    Ellipsis,
    /// A sandbox (Lucide `box`).
    Box,
    /// A cluster service (Lucide `server`).
    Server,
    /// A host without a brand monogram (Lucide `globe`).
    Globe,
    /// More filters (Lucide `list-filter`).
    Filter,
    /// Pause the live view (Lucide `pause`).
    Pause,
    /// Resume the live view (Lucide `play`).
    Play,
    /// The factory mark; also `assets/logo.svg`.
    Logo,
    /// A tool call (Lucide `wrench`).
    Wrench,
    /// A row written by the system, not the agent (Lucide `bot-message-square`).
    BotMessage,
    /// Generic Git (branch).
    Git,
    /// GitHub, used only to identify the forge next to a repository.
    GitHub,
}

/// Inner elements of the factory mark (Lucide `factory`, ISC licence); `assets/logo.svg` has the
/// same paths.
const LOGO_BODY: &str = "<path d=\"M2 20a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V8l-7 5V8l-7 5V4a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2Z\"/><path d=\"M17 18h1\"/><path d=\"M12 18h1\"/><path d=\"M7 18h1\"/>";

impl Icon {
    pub const ALL: &'static [Self] = &[
        Self::Plus,
        Self::Tasks,
        Self::Projects,
        Self::Personas,
        Self::Credentials,
        Self::Shield,
        Self::Activity,
        Self::Inbox,
        Self::LogOut,
        Self::PanelRight,
        Self::Close,
        Self::Search,
        Self::Check,
        Self::ChevronDown,
        Self::ChevronLeft,
        Self::ChevronRight,
        Self::Pencil,
        Self::PullRequest,
        Self::Issue,
        Self::Send,
        Self::ArrowUp,
        Self::Link,
        Self::Trash,
        Self::Ellipsis,
        Self::Box,
        Self::Server,
        Self::Globe,
        Self::Filter,
        Self::Pause,
        Self::Play,
        Self::Logo,
        Self::Wrench,
        Self::BotMessage,
        Self::Git,
        Self::GitHub,
    ];

    /// Inner SVG elements of the 24x24 icon.
    const fn body(self) -> &'static str {
        match self {
            Self::Wrench => {
                "<path d=\"M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.106-3.105c.32-.322.863-.22.983.218a6 6 0 0 1-8.259 7.057l-7.91 7.91a1 1 0 0 1-2.999-3l7.91-7.91a6 6 0 0 1 7.057-8.259c.438.12.54.662.219.984z\"/>"
            }
            Self::BotMessage => {
                "<path d=\"M12 6V2H8\"/><path d=\"m8 18-4 4V8a2 2 0 0 1 2-2h12a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2Z\"/><path d=\"M2 12h2\"/><path d=\"M9 11v2\"/><path d=\"M15 11v2\"/><path d=\"M20 12h2\"/>"
            }
            Self::Plus => "<path d=\"M5 12h14\"/><path d=\"M12 5v14\"/>",
            Self::Tasks => {
                "<rect x=\"3\" y=\"5\" width=\"6\" height=\"6\" rx=\"1\"/><path d=\"m3 17 2 2 4-4\"/><path d=\"M13 6h8\"/><path d=\"M13 12h8\"/><path d=\"M13 18h8\"/>"
            }
            Self::Projects => {
                "<path d=\"M9 20H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H20a2 2 0 0 1 2 2v5\"/><circle cx=\"13\" cy=\"12\" r=\"2\"/><path d=\"M18 19c-2.8 0-5-2.2-5-5v8\"/><circle cx=\"20\" cy=\"19\" r=\"2\"/>"
            }
            Self::Personas => {
                "<path d=\"M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2\"/><circle cx=\"9\" cy=\"7\" r=\"4\"/><path d=\"M22 21v-2a4 4 0 0 0-3-3.87\"/><path d=\"M16 3.13a4 4 0 0 1 0 7.75\"/>"
            }
            Self::Credentials => {
                "<path d=\"M2.586 17.414A2 2 0 0 0 2 18.828V21a1 1 0 0 0 1 1h3a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h1a1 1 0 0 0 1-1v-1a1 1 0 0 1 1-1h.172a2 2 0 0 0 1.414-.586l.814-.814a6.5 6.5 0 1 0-4-4z\"/><circle cx=\"16.5\" cy=\"7.5\" r=\".5\" fill=\"currentColor\"/>"
            }
            Self::Shield => {
                "<path d=\"M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z\"/><path d=\"m9 12 2 2 4-4\"/>"
            }
            Self::Activity => {
                "<path d=\"M22 12h-2.48a2 2 0 0 0-1.93 1.46l-2.35 8.36a.25.25 0 0 1-.48 0L9.24 2.18a.25.25 0 0 0-.48 0l-2.35 8.36A2 2 0 0 1 4.49 12H2\"/>"
            }
            Self::Inbox => {
                "<polyline points=\"22 12 16 12 14 15 10 15 8 12 2 12\"/><path d=\"M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z\"/>"
            }
            Self::LogOut => {
                "<path d=\"M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4\"/><polyline points=\"16 17 21 12 16 7\"/><line x1=\"21\" x2=\"9\" y1=\"12\" y2=\"12\"/>"
            }
            Self::PanelRight => {
                "<rect width=\"18\" height=\"18\" x=\"3\" y=\"3\" rx=\"2\"/><path d=\"M15 3v18\"/>"
            }
            Self::Close => "<path d=\"M18 6 6 18\"/><path d=\"m6 6 12 12\"/>",
            Self::Search => "<circle cx=\"11\" cy=\"11\" r=\"8\"/><path d=\"m21 21-4.3-4.3\"/>",
            Self::Check => "<path d=\"M20 6 9 17l-5-5\"/>",
            Self::ChevronDown => "<path d=\"m6 9 6 6 6-6\"/>",
            Self::ChevronLeft => "<path d=\"m15 18-6-6 6-6\"/>",
            Self::ChevronRight => "<path d=\"m9 18 6-6-6-6\"/>",
            Self::Pencil => {
                "<path d=\"M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z\"/><path d=\"m15 5 4 4\"/>"
            }
            Self::PullRequest => {
                "<circle cx=\"18\" cy=\"18\" r=\"3\"/><circle cx=\"6\" cy=\"6\" r=\"3\"/><path d=\"M13 6h3a2 2 0 0 1 2 2v7\"/><line x1=\"6\" x2=\"6\" y1=\"9\" y2=\"21\"/>"
            }
            Self::Issue => {
                "<circle cx=\"12\" cy=\"12\" r=\"10\"/><circle cx=\"12\" cy=\"12\" r=\"1\"/>"
            }
            Self::Send => {
                "<path d=\"M14.536 21.686a.5.5 0 0 0 .937-.024l6.5-19a.496.496 0 0 0-.635-.635l-19 6.5a.5.5 0 0 0-.024.937l7.93 3.18a2 2 0 0 1 1.112 1.11z\"/><path d=\"m21.854 2.147-10.94 10.939\"/>"
            }
            Self::ArrowUp => "<path d=\"M12 19V5\"/><path d=\"m5 12 7-7 7 7\"/>",
            Self::Logo => LOGO_BODY,
            Self::Git => {
                "<line x1=\"6\" x2=\"6\" y1=\"3\" y2=\"15\"/><circle cx=\"18\" cy=\"6\" r=\"3\"/><circle cx=\"6\" cy=\"18\" r=\"3\"/><path d=\"M18 9a9 9 0 0 1-9 9\"/>"
            }
            // GitHub mark (simple-icons, CC0): filled, not stroked.
            Self::GitHub => {
                "<path fill=\"currentColor\" stroke=\"none\" d=\"M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12\"/>"
            }
            Self::Trash => {
                "<path d=\"M3 6h18\"/><path d=\"M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6\"/><path d=\"M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2\"/>"
            }
            Self::Ellipsis => {
                "<circle cx=\"12\" cy=\"12\" r=\"1\"/><circle cx=\"19\" cy=\"12\" r=\"1\"/><circle cx=\"5\" cy=\"12\" r=\"1\"/>"
            }
            Self::Box => {
                "<path d=\"M21 8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z\"/><path d=\"m3.3 7 8.7 5 8.7-5\"/><path d=\"M12 22V12\"/>"
            }
            Self::Server => {
                "<rect width=\"20\" height=\"8\" x=\"2\" y=\"2\" rx=\"2\" ry=\"2\"/><rect width=\"20\" height=\"8\" x=\"2\" y=\"14\" rx=\"2\" ry=\"2\"/><line x1=\"6\" x2=\"6.01\" y1=\"6\" y2=\"6\"/><line x1=\"6\" x2=\"6.01\" y1=\"18\" y2=\"18\"/>"
            }
            Self::Globe => {
                "<circle cx=\"12\" cy=\"12\" r=\"10\"/><path d=\"M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20\"/><path d=\"M2 12h20\"/>"
            }
            Self::Filter => "<path d=\"M3 6h18\"/><path d=\"M7 12h10\"/><path d=\"M10 18h4\"/>",
            Self::Pause => "<path d=\"M9 5v14\"/><path d=\"M15 5v14\"/>",
            Self::Play => {
                "<path d=\"M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z\"/>"
            }
            Self::Link => {
                "<path d=\"M9 17H7A5 5 0 0 1 7 7h2\"/><path d=\"M15 7h2a5 5 0 1 1 0 10h-2\"/><line x1=\"8\" x2=\"16\" y1=\"12\" y2=\"12\"/>"
            }
        }
    }
}

/// The icon of a forge integration (`None`: no integration, generic Git). Named by `label` for
/// assistive technology and as tooltip.
pub fn forge_icon(kind: Option<&df_core::ForgeKind>) -> Markup {
    let (glyph, name, data) = match kind {
        Some(df_core::ForgeKind::Forgejo) => return forgejo_logo(),
        Some(df_core::ForgeKind::GitHub) => (Icon::GitHub, "GitHub", "github"),
        Some(df_core::ForgeKind::Other(name)) => (Icon::Git, name.as_str(), "git"),
        None => (Icon::Git, "Git", "git"),
    };
    html! {
        span class="inline-flex shrink-0" data-forge=(data) role="img" aria-label=(name) title=(name) { (icon(glyph)) }
    }
}

/// The Forgejo logo as drawn on the design canvas: a git graph in Forgejo's orange and red, so it
/// does not follow the text colour. Forgejo logo (c) Forgejo contributors, CC BY-SA 4.0,
/// <https://forgejo.org>; shown only to identify the forge.
fn forgejo_logo() -> Markup {
    html! {
        span class="inline-flex shrink-0" data-forge="forgejo" role="img" aria-label="Forgejo" title="Forgejo" {
            svg class="size-5 shrink-0" viewBox="0 0 212 212" aria-hidden="true" {
                g fill="none" stroke-width="25" {
                    path d="M58 168V70a50 50 0 0 1 50-50h20" stroke="#ff6600" {}
                    path d="M58 168v-30a50 50 0 0 1 50-50h20" stroke="#d40000" {}
                    circle cx="142" cy="20" r="18" stroke="#ff6600" {}
                    circle cx="142" cy="88" r="18" stroke="#d40000" {}
                    circle cx="58" cy="180" r="18" stroke="#d40000" {}
                }
            }
        }
    }
}

/// The factory mark in the text colour, sized by `class`. Decorative like [`icon`].
pub fn logo(class: &str) -> Markup {
    html! {
        svg class=(class) viewBox="0 0 24 24" fill="none" stroke="currentColor"
            stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" {
            (PreEscaped(Icon::Logo.body()))
        }
    }
}

/// An inline icon in the text colour. Decorative: it is hidden from assistive technology, so the
/// control that holds it needs its own accessible name.
pub fn icon(icon: Icon) -> Markup {
    html! {
        svg class="size-5 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor"
            stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" {
            (PreEscaped(icon.body()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forge_icons_are_named_and_marked_by_kind() {
        use df_core::ForgeKind;
        let forgejo = forge_icon(Some(&ForgeKind::Forgejo)).into_string();
        assert!(
            forgejo.contains("data-forge=\"forgejo\"")
                && forgejo.contains("aria-label=\"Forgejo\""),
            "{forgejo}"
        );
        // The logo of the canvas: its own viewBox and brand colors.
        assert!(
            forgejo.contains("viewBox=\"0 0 212 212\"")
                && forgejo.contains("#ff6600")
                && forgejo.contains("#d40000")
        );
        let github = forge_icon(Some(&ForgeKind::GitHub)).into_string();
        assert!(
            github.contains("data-forge=\"github\"") && github.contains("fill=\"currentColor\"")
        );
        let other = forge_icon(Some(&ForgeKind::Other("gitea".into()))).into_string();
        assert!(other.contains("data-forge=\"git\"") && other.contains("title=\"gitea\""));
        let none = forge_icon(None).into_string();
        assert!(none.contains("data-forge=\"git\"") && none.contains("aria-label=\"Git\""));
    }

    #[test]
    fn every_icon_is_decorative_inline_svg_without_external_references() {
        for glyph in Icon::ALL {
            let html = icon(*glyph).into_string();
            assert!(
                html.contains("aria-hidden=\"true\"") && html.contains("currentColor"),
                "{glyph:?}"
            );
            assert!(
                html.contains("<path")
                    || html.contains("<circle")
                    || html.contains("<rect")
                    || html.contains("<line"),
                "{glyph:?}"
            );
            assert!(!html.contains("http"), "{glyph:?}");
        }
    }
}
