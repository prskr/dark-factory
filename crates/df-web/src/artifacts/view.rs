//! Markup of artifacts: cards, the artifact view with its header and the details tab heading.

use chrono::{DateTime, Utc};
use df_db::artifacts::ArtifactMeta;
use maud::{Markup, html};

use crate::components::{ICON_BUTTON_CLASS, INITIAL_DETAILS_TAB, Icon, icon};
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
pub(super) enum Place {
    /// The split view of the task page: with a close action.
    Split,
    /// The artifact page.
    Page,
}

pub(super) fn artifact_url(task_key: &str, name: &str) -> String {
    format!("/tasks/{task_key}/artifacts/{name}")
}

fn is_image(meta: &ArtifactMeta) -> bool {
    meta.kind == "image"
}

fn is_markdown(meta: &ArtifactMeta) -> bool {
    !is_image(meta) && meta.mime_type == "text/markdown"
}

/// The label of an artifact's type: `Markdown`, `Text`, or the image format.
pub(super) fn type_label(meta: &ArtifactMeta) -> &'static str {
    match meta.mime_type.as_str() {
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

fn time(at: DateTime<Utc>) -> Markup {
    html! { time datetime=(at.to_rfc3339()) { (at.format("%b %d %H:%M UTC")) } }
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
                @let meta = &card.meta;
                li {
                    a href=(artifact_url(task_key, &meta.name)) "x-on:click.prevent"={"open('" (meta.name) "')"}
                        class="block overflow-hidden rounded-xl border border-line bg-panel hover:border-line-strong" {
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
                                (type_label(meta)) " \u{b7} " (size_label(meta.size)) " \u{b7} " (time(meta.updated_at))
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The heading of the details column: "Details" while the task has no artifact, else the tab bar.
/// `oob` makes it replace the heading in the page (response of the tab fragment).
pub(crate) fn details_heading(count: usize, oob: bool) -> Markup {
    html! {
        div id="ws-details-heading" hx-swap-oob=[oob.then_some("outerHTML")] class="min-w-0" {
            @if count == 0 {
                h2 class="text-base font-semibold" { "Details" }
            } @else {
                // The `tabs` component sits on the details panel (`details_panel_with_heading`).
                div role="tablist" aria-label="Details" "x-on:keydown"="move($event)" x-init="adopt()" class="flex items-center gap-4 text-sm font-medium" {
                    (tab_button("overview", "panel-overview", html! { "Overview" }))
                    (tab_button("artifacts", "panel-artifacts", html! { "Artifacts " span class="rounded-full bg-sunk px-1.5 text-xs" { (count) } }))
                }
            }
        }
    }
}

/// A tab of the details column, for the `tabs` component (`scripts/tabs.ts`): the first one is
/// selected until Alpine takes over.
fn tab_button(id: &str, panel: &str, label: Markup) -> Markup {
    let first = id == INITIAL_DETAILS_TAB;
    html! {
        button type="button" role="tab" id={"tab-" (id)} data-tab=(id) aria-controls=(panel)
            aria-selected=(first) tabindex=(if first { "0" } else { "-1" })
            ":aria-selected"={"tab === '" (id) "'"} ":tabindex"={"tab === '" (id) "' ? 0 : -1"}
            "x-on:click"={"tab = '" (id) "'"} ":class"={"tab === '" (id) "' ? 'text-ink' : 'text-muted'"} { (label) }
    }
}

/// The Artifacts tab: the cards. It reloads itself on `artifacts-changed` (a `write_artifact` row
/// arrived in the transcript).
pub(crate) fn tab_panel(task_key: &str, artifacts: &[ArtifactCard]) -> Markup {
    // A tabpanel only while the tab bar is shown; the tab bar adopts the panels when it arrives.
    let tabbed = !artifacts.is_empty();
    html! {
        div id="panel-artifacts" role=[tabbed.then_some("tabpanel")] aria-labelledby=[tabbed.then_some("tab-artifacts")] class="ws-details-full ws-artifacts min-h-0 flex-1 overflow-y-auto"
            hx-get={"/tasks/" (task_key) "/artifacts"} hx-trigger="artifacts-changed from:body"
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
                        (type_label(meta)) " \u{b7} " (size_label(meta.size)) " \u{b7} "
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
                    div x-show="!source" class="md-body" {
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

/// The transcript row of a successful `write_artifact` call; it opens the artifact's current
/// version. `name` must be a valid artifact name (it is written into an attribute and a script
/// string). `data-artifact-written` makes the page reload the tab and an open artifact.
pub(crate) fn artifact_row(id: &str, name: &str) -> Markup {
    let (label, glyph) = match extension(name) {
        "md" => ("Markdown", Icon::FileText),
        "png" | "jpg" | "jpeg" | "webp" | "gif" => ("Image", Icon::ImageFile),
        _ => ("Text", Icon::FileText),
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
