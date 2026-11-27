//! Markup of artifacts: cards, the artifact view with its header and the details tab heading.

use df_db::artifacts::ArtifactMeta;
use df_proto::transcript::v1::{ArtifactKind, ArtifactWritten};
use jiff::Timestamp;
use maud::{Markup, html};

use crate::components::{
    ICON_BUTTON_CLASS, INITIAL_DETAILS_TAB, Icon, MarkerInset, TAB_CLASS, icon, tab_marker,
};
use crate::htmx::Region;
use crate::markdown::{ImageArtifacts, highlight_by_extension, render_with_images};

/// A card shows at most this many lines of a text artifact, and at most [`PREVIEW_BYTES`].
const PREVIEW_LINES: usize = 8;
const PREVIEW_BYTES: usize = 600;

/// An artifact with the start of its text, for the card.
pub(crate) struct ArtifactCard {
    pub meta: ArtifactMeta,
    pub preview: Vec<u8>,
}

/// Where an artifact is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    /// The split view of the task page: with a close action.
    Split,
    /// The artifact page.
    Page,
}

pub(super) fn artifact_url(task_key: &str, name: &str) -> String {
    format!("{}/artifacts/{name}", crate::layout::item_path(task_key))
}

fn is_image(meta: &ArtifactMeta) -> bool {
    meta.kind == "image"
}

fn is_markdown(meta: &ArtifactMeta) -> bool {
    !is_image(meta) && meta.mime_type == "text/markdown"
}

/// The label of an artifact's type: `Markdown`, `Text`, or the image format.
pub(super) fn type_label(mime_type: &str) -> &'static str {
    match mime_type {
        "text/markdown" => "Markdown",
        "image/png" => "PNG",
        "image/jpeg" => "JPEG",
        "image/webp" => "WebP",
        "image/gif" => "GIF",
        _ => "Text",
    }
}

/// `1.2 KB` style size.
pub(super) fn size_label(bytes: i64) -> String {
    #[expect(clippy::cast_precision_loss, reason = "a display value")]
    let value = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", value / 1024.0)
    } else {
        format!("{:.2} MB", value / (1024.0 * 1024.0))
    }
}

fn time(at: Timestamp) -> Markup {
    html! { time datetime=(at.to_string()) { (at.strftime("%b %d %H:%M UTC").to_string()) } }
}

fn dimensions(meta: &ArtifactMeta) -> Option<String> {
    Some(format!("{} \u{d7} {} px", meta.width?, meta.height?))
}

/// The extension of `name` (`rs` for `sketch.rs`), if it has one.
fn extension(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext,
        _ => "",
    }
}

/// The first lines of a text for a card, plain.
fn preview_text(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    for line in text.lines().take(PREVIEW_LINES) {
        if out.len() + line.len() > PREVIEW_BYTES {
            let room = PREVIEW_BYTES.saturating_sub(out.len());
            let mut end = room.min(line.len());
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            out.push_str(line.get(..end).unwrap_or_default());
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The cards of a task's artifacts, newest update first (the order of `artifacts`). Opening a
/// card calls `open(name)` of the `artifactSplit` component; without it the link opens the page.
pub(crate) fn cards(task_key: &str, artifacts: &[ArtifactCard]) -> Markup {
    html! {
        ul id="artifact-cards" class="space-y-3 p-4" {
            @for card in artifacts {
                li {
                    a href=(artifact_url(task_key, &card.meta.name)) "x-on:click.prevent"={"open('" (card.meta.name) "')"}
                        class="block overflow-hidden rounded-xl border border-line bg-panel hover:border-line-strong" {
                        (card_body(task_key, card))
                    }
                }
            }
        }
    }
}

/// The cards of an archived chat, several per row. `open_url` is where the card's artifact is
/// shown: the link swaps it into the modal body.
pub(crate) fn card_grid(
    task_key: &str,
    artifacts: &[ArtifactCard],
    open_url: impl Fn(&str) -> String,
) -> Markup {
    html! {
        ul class="grid grid-cols-1 gap-3 p-4 sm:grid-cols-2 xl:grid-cols-3" {
            @for card in artifacts {
                @let url = open_url(&card.meta.name);
                li {
                    a href=(url) hx-get=(url) hx-target=(Region::ArchiveBody.selector()) hx-swap="outerHTML"
                        class="block overflow-hidden rounded-xl border border-line bg-panel hover:border-line-strong" {
                        (card_body(task_key, card))
                    }
                }
            }
        }
    }
}

/// The preview and the caption of a card.
fn card_body(task_key: &str, card: &ArtifactCard) -> Markup {
    let meta = &card.meta;
    html! {
        @if is_image(meta) {
            img src={(artifact_url(task_key, &meta.name)) "/raw"} alt="" loading="lazy"
                class="h-28 w-full bg-sunk object-cover";
        } @else {
            pre class="h-28 overflow-hidden bg-sunk p-3 font-mono text-xs text-muted" { (preview_text(&card.preview)) }
        }
        div class="space-y-0.5 p-3" {
            div class="flex items-center gap-2 text-sm font-medium" {
                span class="text-muted" { (icon(if is_image(meta) { Icon::ImageFile } else { Icon::FileText })) }
                span class="min-w-0 flex-1 truncate" { (meta.name) }
            }
            div class="text-xs text-faint" {
                (type_label(&meta.mime_type)) " \u{b7} " (size_label(meta.size)) " \u{b7} " (time(meta.updated_at))
            }
        }
    }
}

/// The heading of the details column: "Details" while the task has neither subtasks nor an
/// artifact, else the tab bar (Overview, Subtasks, Artifacts: the last two when there are any).
/// `oob` makes it replace the heading in the page (response of the tab fragment).
pub(crate) fn details_heading(subtasks: usize, artifacts: usize, oob: bool) -> Markup {
    html! {
        div id=(Region::WsDetailsHeading.id()) hx-swap-oob=[oob.then_some("outerHTML")] class="min-w-0" {
            @if subtasks == 0 && artifacts == 0 {
                h2 class="text-base font-semibold" { "Details" }
            } @else {
                // The `tabs` component sits on the details panel (`details_panel_with_heading`).
                div role="tablist" aria-label="Details" "x-on:keydown"="move($event)" x-init="adopt()" class="flex items-center gap-4 text-sm" {
                    (tab_button("overview", "panel-overview", html! { "Overview" }))
                    @if subtasks > 0 {
                        (tab_button("subtasks", "panel-subtasks", html! { "Subtasks " (count(subtasks)) }))
                    }
                    @if artifacts > 0 {
                        (tab_button("artifacts", "panel-artifacts", html! { "Artifacts " (count(artifacts)) }))
                    }
                }
            }
        }
    }
}

/// The heading of the details column of a chat: always the tab bar with the three tabs, the last
/// two with their counts.
pub(crate) fn chat_details_heading(subtasks: usize, artifacts: usize, oob: bool) -> Markup {
    html! {
        div id=(Region::WsDetailsHeading.id()) hx-swap-oob=[oob.then_some("outerHTML")] class="min-w-0" {
            div role="tablist" aria-label="Details" "x-on:keydown"="move($event)" x-init="adopt()" class="flex items-center gap-4 text-sm" {
                (tab_button("overview", "panel-overview", html! { "Overview" }))
                (tab_button("subtasks", "panel-subtasks", html! { "Subtasks " (count(subtasks)) }))
                (tab_button("artifacts", "panel-artifacts", html! { "Artifacts " (count(artifacts)) }))
            }
        }
    }
}

/// A tab of the details column, for the `tabs` component (`scripts/tabs.ts`): the first one is
/// selected until Alpine takes over.
fn tab_button(id: &str, panel: &str, label: Markup) -> Markup {
    let first = id == INITIAL_DETAILS_TAB;
    html! {
        // The selected tab: medium weight and a 2 px accent line, like the settings page tabs
        // (`marker_class` in `config_pages.rs`); the others are muted and regular.
        button type="button" role="tab" id={"tab-" (id)} data-tab=(id) aria-controls=(panel)
            class={(TAB_CLASS) " px-2 lg:min-h-8"}
            aria-selected=(first) tabindex=(if first { "0" } else { "-1" })
            ":aria-selected"={"tab === '" (id) "'"} ":tabindex"={"tab === '" (id) "' ? 0 : -1"}
            "x-on:click"={"tab = '" (id) "'"} {
            (tab_marker(MarkerInset::Narrow))
            (label)
        }
    }
}

/// The count after a tab label: a plain muted number.
fn count(n: usize) -> Markup {
    html! { span class="ml-1.5 text-[13px] font-normal text-muted" { (n) } }
}

/// The Artifacts tab: the cards. It reloads itself on `artifacts-changed` (a `write_artifact` row
/// arrived in the transcript).
pub(crate) fn tab_panel(task_key: &str, artifacts: &[ArtifactCard]) -> Markup {
    // A tabpanel only while the tab bar is shown; the tab bar adopts the panels when it arrives.
    let tabbed = !artifacts.is_empty();
    html! {
        div id="panel-artifacts" role=[tabbed.then_some("tabpanel")] aria-labelledby=[tabbed.then_some("tab-artifacts")] class="ws-details-full ws-artifacts min-h-0 flex-1 overflow-y-auto"
            hx-get={(crate::layout::item_path(task_key)) "/artifacts"} hx-trigger="artifacts-changed from:body"
            hx-target="this" hx-swap="innerHTML" {
            (cards(task_key, artifacts))
        }
    }
}

/// An artifact: header and the rendered content. `images` are the image artifacts of the task
/// that a Markdown artifact may show inline.
pub(super) fn artifact_view(
    task_key: &str,
    meta: &ArtifactMeta,
    content: &[u8],
    images: &[String],
    place: Place,
) -> Markup {
    let url = artifact_url(task_key, &meta.name);
    let text = String::from_utf8_lossy(content);
    html! {
        div class="flex min-h-0 flex-1 flex-col" x-data="{ source: false }" {
            div class="flex items-center gap-2 border-b border-line px-4 py-3" {
                div class="min-w-0 flex-1" {
                    h2 class="truncate text-base font-semibold" { (meta.name) }
                    p class="truncate text-xs text-faint" {
                        (type_label(&meta.mime_type)) " \u{b7} " (size_label(meta.size)) " \u{b7} "
                        @if let Some(size) = dimensions(meta) { (size) " \u{b7} " }
                        "updated " (time(meta.updated_at))
                    }
                }
                @if is_markdown(meta) {
                    div role="group" aria-label="View" class="flex rounded-lg bg-sunk p-0.5 text-sm" {
                        button type="button" "x-on:click"="source = false" ":aria-pressed"="!source"
                            ":class"="source ? 'text-muted' : 'rounded-md bg-panel text-ink'" class="px-2.5 py-1" { "Preview" }
                        button type="button" "x-on:click"="source = true" ":aria-pressed"="source"
                            ":class"="source ? 'rounded-md bg-panel text-ink' : 'text-muted'" class="px-2.5 py-1" { "Source" }
                    }
                }
                @if place == Place::Split {
                    a href=(url) aria-label="Open full page" title="Open full page" class=(ICON_BUTTON_CLASS) { (icon(Icon::Maximize)) }
                    button type="button" aria-label="Close artifact" "x-on:click"="close()" class=(ICON_BUTTON_CLASS) { (icon(Icon::Close)) }
                }
            }
            div class="min-h-0 flex-1 overflow-y-auto p-4" {
                @if is_image(meta) {
                    img src={(url) "/raw"} alt=(meta.name) class="max-w-full";
                } @else if is_markdown(meta) {
                    div x-show="!source" class="md-body md-page" {
                        (render_with_images(&text, &ImageArtifacts { task_key, names: images }))
                    }
                    div x-show="source" x-cloak { (highlight_by_extension("md", &text)) }
                } @else {
                    (highlight_by_extension(extension(&meta.name), &text))
                }
            }
        }
    }
}

/// The argument of `x-data="artifactSplit(...)"`, escaped by maud where it is written.
pub(crate) fn split_args(task_key: &str) -> String {
    format!(
        "artifactSplit({})",
        serde_json::json!({
            "key": task_key,
            "path": crate::layout::item_path(task_key),
            "splitKey": "df.artifact.split",
            "defaultFraction": 0.45,
            "minPane": 320,
            "minTranscript": 360,
        })
    )
}

/// The divider and the pane of the split view next to the transcript; `artifactSplit` fills the
/// pane with the artifact view. Below 768 px opening an artifact opens its page instead.
pub(crate) fn split_pane() -> Markup {
    html! {
        div role="separator" aria-orientation="vertical" aria-label="Resize artifact" tabindex="0" x-show="openName" x-cloak
            "x-on:pointerdown"="dragStart($event)" "x-on:pointermove"="dragMove($event)"
            "x-on:pointerup"="dragEnd()" "x-on:pointercancel"="dragEnd()" "x-on:keydown"="dividerKey($event)"
            class="w-1 shrink-0 cursor-col-resize touch-none bg-line hover:bg-line-strong focus-visible:bg-accent max-md:hidden" {}
        section id="artifact-pane" aria-label="Artifact" x-show="openName" x-cloak ":style"="paneStyle()"
            class="flex min-w-0 shrink-0 flex-col bg-panel max-md:hidden" {}
    }
}

/// The transcript row of an artifact-written event; it opens the artifact's current version.
/// `written.name` must be a valid artifact name (it is written into an attribute and a script
/// string). `data-artifact-written` makes the page reload the tab and an open artifact. An
/// unspecified kind is shown as text.
pub(crate) fn artifact_row(id: &str, written: &ArtifactWritten) -> Markup {
    let name = written.name.as_str();
    let label = type_label(&written.mime_type);
    let glyph = if written.kind() == ArtifactKind::Image {
        Icon::ImageFile
    } else {
        Icon::FileText
    };
    html! {
        div id=(id) data-author="agent" data-tool data-artifact-written=(name) class="ml-11" {
            button type="button" "x-on:click"={"open('" (name) "')"}
                class="flex min-h-11 w-full items-center gap-2 text-left text-sm text-muted hover:text-ink lg:min-h-8" {
                span { (icon(glyph)) }
                span class="min-w-0 truncate font-medium text-ink" { (name) }
                span class="text-xs text-faint" { "\u{b7} " (label) }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_artifact_time_shows_utc_text_and_an_rfc3339_datetime_attribute() {
        let at: Timestamp = "2026-10-23T10:42:00Z".parse().unwrap();
        assert_eq!(
            time(at).into_string(),
            "<time datetime=\"2026-10-23T10:42:00Z\">Oct 23 10:42 UTC</time>"
        );
    }
}
