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
    LogOut,
    PanelRight,
    Close,
    Search,
    Check,
    ChevronDown,
    ChevronLeft,
    Pencil,
    PullRequest,
    Issue,
    Send,
    Link,
}

impl Icon {
    pub const ALL: &'static [Self] = &[
        Self::Plus,
        Self::Tasks,
        Self::Projects,
        Self::Personas,
        Self::Credentials,
        Self::LogOut,
        Self::PanelRight,
        Self::Close,
        Self::Search,
        Self::Check,
        Self::ChevronDown,
        Self::ChevronLeft,
        Self::Pencil,
        Self::PullRequest,
        Self::Issue,
        Self::Send,
        Self::Link,
    ];

    /// Inner SVG elements of the 24x24 icon.
    const fn body(self) -> &'static str {
        match self {
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
            Self::Link => {
                "<path d=\"M9 17H7A5 5 0 0 1 7 7h2\"/><path d=\"M15 7h2a5 5 0 1 1 0 10h-2\"/><line x1=\"8\" x2=\"16\" y1=\"12\" y2=\"12\"/>"
            }
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
