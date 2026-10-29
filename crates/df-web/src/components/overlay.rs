//! Dialogs and panels: a native `<dialog>` that is a bottom sheet on phones, and the details
//! panel of the task workspace (bottom sheet, slide-over or column by width).

use maud::{Markup, html};

use super::actions::ICON_BUTTON_CLASS;
use super::icons::{Icon, icon};

/// How a [`dialog_with`] looks. All are bottom sheets or centered cards that open themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogStyle {
    /// A card with title and close button; a bottom sheet below 768 px.
    Sheet,
    /// Like `Sheet`, but a full-height sheet below 768 px (forms with many fields).
    Form,
    /// No frame: the body is the card (the new-task composer). The body brings its own close
    /// button ([`dialog_close`]).
    Bare,
}

/// A modal dialog opened as soon as it is inserted into the page (`showModal()` provides focus
/// trapping and Escape) and removed when it closes. Below 768 px it is a bottom sheet.
pub fn dialog(id: &str, title: &str, body: Markup) -> Markup {
    dialog_with(id, title, DialogStyle::Sheet, body)
}

/// A dialog in the given [`DialogStyle`].
pub fn dialog_with(id: &str, title: &str, style: DialogStyle, body: Markup) -> Markup {
    let title_id = format!("{id}-title");
    let class = match style {
        DialogStyle::Sheet => {
            "fixed inset-x-0 bottom-0 m-0 mt-auto w-full max-w-none rounded-t-2xl border border-line bg-panel p-0 text-ink backdrop:bg-black/50 md:inset-0 md:m-auto md:h-fit md:max-w-xl md:rounded-2xl"
        }
        DialogStyle::Form => {
            "fixed inset-0 m-0 h-dvh max-h-none w-full max-w-none overflow-y-auto border border-line bg-panel p-0 text-ink backdrop:bg-black/50 md:m-auto md:h-fit md:max-h-[90dvh] md:max-w-xl md:rounded-2xl"
        }
        DialogStyle::Bare => {
            "fixed inset-x-0 bottom-0 m-0 mt-auto w-full max-w-none overflow-visible bg-transparent p-3 text-ink backdrop:bg-black/50 md:inset-0 md:m-auto md:h-fit md:max-w-xl md:p-0"
        }
    };
    html! {
        dialog id=(id) aria-labelledby=(title_id) x-data x-init="$el.showModal()" "x-on:close"="$el.remove()"
            "x-on:modal-close.window"="$el.close()" "x-on:click"="$event.target === $el && $el.close()" class=(class) {
            @if style == DialogStyle::Bare {
                h2 id=(title_id) class="sr-only" { (title) }
                (body)
            } @else {
                div class="flex items-center justify-between gap-2 px-4 pt-3 md:px-5" {
                    h2 id=(title_id) class="text-base font-semibold" { (title) }
                    (dialog_close())
                }
                div class="px-4 pb-4 md:px-5" { (body) }
            }
        }
    }
}

/// Like [`ICON_BUTTON_CLASS`] without a background on hover (the button sits on a card or on the
/// backdrop).
const CLOSE_BUTTON_CLASS: &str = "inline-flex size-11 shrink-0 items-center justify-center rounded-lg text-muted hover:text-ink lg:size-9";

/// The close button of a dialog; closing removes the dialog. A plain button, so it can sit inside
/// the dialog's own form.
pub fn dialog_close() -> Markup {
    html! {
        button type="button" class=(CLOSE_BUTTON_CLASS) aria-label="Close" "x-on:click"="$el.closest('dialog').close()" { (icon(Icon::Close)) }
    }
}

/// The details of a workspace. It expects the parent to hold the workspace Alpine state
/// (`assets/workspace.js`): `details` (open or not), `collapsed` with
/// `setCollapsed(value)` and, for the phone sheet, `sheet`, `dragging` and the `sheetToggle` /
/// `sheetDrag*` methods; from 1280 px it is a permanent column that
/// collapses to a strip. The live region inside `body` renders the strip (`ws-strip`) and marks
/// what the strip hides with `ws-details-full`.
pub fn details_panel(title: &str, body: Markup) -> Markup {
    html! {
        aside id="ws-details" class="ws-details" aria-label=(title) ":data-open"="details" ":data-size"="sheet" ":data-dragging"="dragging" {
            // Phones: the grey handle resizes the sheet (drag, or tap / keyboard to toggle).
            button type="button" aria-label="Resize details" ":aria-expanded"="sheet === 'full'"
                "x-on:click"="sheetToggle()" "x-on:pointerdown"="sheetDragStart($event)" "x-on:pointermove"="sheetDragMove($event)"
                "x-on:pointerup"="sheetDragEnd($event)" "x-on:pointercancel"="sheetDragCancel($event)"
                class="flex min-h-11 w-full shrink-0 touch-none justify-center pt-2 pb-1 md:hidden" {
                span aria-hidden="true" class="h-[5px] w-10 rounded-full bg-line" {}
            }
            // No display utility on the header: the stylesheet hides it in the collapsed strip.
            div class="ws-details-head" {
                h2 class="text-base font-semibold" { (title) }
                div class="flex items-center gap-1" {
                    button type="button" class={(ICON_BUTTON_CLASS) " xl:hidden"} aria-label="Close details" "x-on:click"="details = false" { (icon(Icon::Close)) }
                    button type="button" class={(ICON_BUTTON_CLASS) " max-xl:hidden"} aria-label="Hide details" "x-on:click"="setCollapsed(true)" { (icon(Icon::PanelRight)) }
                }
            }
            div class="ws-details-body min-h-0 flex-1 overflow-y-auto md:flex md:flex-col" { (body) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_is_native_labelled_and_closable() {
        let html = dialog("composer", "New task", html! { p { "x" } }).into_string();
        assert!(
            html.starts_with("<dialog id=\"composer\" aria-labelledby=\"composer-title\""),
            "{html}"
        );
        assert!(html.contains("x-init=\"$el.showModal()\""));
        assert!(
            html.contains("aria-label=\"Close\"") && html.contains("closest('dialog').close()")
        );
        assert!(
            html.contains("fixed inset-x-0 bottom-0"),
            "bottom sheet on phones"
        );
    }

    #[test]
    fn details_panel_has_a_name_and_a_close_and_a_hide_button() {
        let html = details_panel("Details", html! { "x" }).into_string();
        assert!(
            html.contains("aria-label=\"Details\"")
                && html.contains("aria-label=\"Close details\"")
                && html.contains("aria-label=\"Hide details\""),
            "{html}"
        );
        assert!(html.contains("size-11"));
        assert!(html.contains("x-on:click=\"setCollapsed(true)\""));
        // Heading left, both controls grouped on the right; the hide button only from 1280 px.
        assert!(
            html.contains("</h2><div class=\"flex items-center gap-1\"><button"),
            "{html}"
        );
        assert!(html.contains("max-xl:hidden") && !html.contains(" hidden xl:inline-flex"));
        // A display utility would beat the stylesheet rule that hides the header when collapsed.
        let head = &html[html.find("class=\"ws-details-head").unwrap()..];
        assert!(head.starts_with("class=\"ws-details-head\""), "{head}");
    }

    #[test]
    fn the_phone_sheet_has_a_resize_handle_for_keyboard_and_pointer() {
        let html = details_panel("Details", html! { "x" }).into_string();
        let start = html.find("aria-label=\"Resize details\"").unwrap();
        let tag = &html[html[..start].rfind("<button").unwrap()..];
        let tag = &tag[..tag.find("</button>").unwrap()];
        assert!(
            tag.contains("type=\"button\"") && tag.contains(":aria-expanded=\"sheet === 'full'\""),
            "{tag}"
        );
        // 44 px touch target below 1024 px (web-ui-shell accessibility).
        assert!(tag.contains("min-h-11"), "{tag}");
        for handler in [
            "x-on:click=\"sheetToggle()\"",
            "x-on:pointerdown=\"sheetDragStart($event)\"",
            "x-on:pointermove=\"sheetDragMove($event)\"",
            "x-on:pointerup=\"sheetDragEnd($event)\"",
            "x-on:pointercancel=\"sheetDragCancel($event)\"",
        ] {
            assert!(tag.contains(handler), "{handler}: {tag}");
        }
        // A 40 x 5 px grey bar, centred, 8 px from the top; touch-action none so the drag is ours.
        assert!(
            tag.contains("touch-none")
                && tag.contains("pt-2")
                && tag.contains("justify-center")
                && tag.contains("md:hidden"),
            "{tag}"
        );
        assert!(tag.contains("h-[5px] w-10 rounded-full bg-line"), "{tag}");
        assert!(html.contains(":data-size=\"sheet\""));
        // The existing close paths are still there.
        assert!(
            html.contains("aria-label=\"Close details\"")
                && html.contains("x-on:click=\"details = false\"")
        );
        let css = crate::app_css().unwrap();
        assert!(
            css.contains("border-top-left-radius:18px"),
            "18 px top radius"
        );
        assert!(css.contains("[data-size=full]"), "full height");
    }

    #[test]
    fn the_dialog_close_button_has_no_background_on_hover() {
        let html = dialog_close().into_string();
        assert!(!html.contains("hover:bg-"), "{html}");
    }

    #[test]
    fn dialog_styles_differ_on_phones() {
        let sheet = dialog_with("d", "T", DialogStyle::Sheet, html! {}).into_string();
        let form = dialog_with("d", "T", DialogStyle::Form, html! {}).into_string();
        let bare = dialog_with("d", "T", DialogStyle::Bare, html! { "body" }).into_string();
        for html in [&sheet, &form, &bare] {
            assert!(
                html.contains("x-on:click=\"$event.target === $el &amp;&amp; $el.close()\""),
                "backdrop closes: {html}"
            );
        }
        assert!(sheet.contains("rounded-t-2xl") && sheet.contains("aria-label=\"Close\""));
        assert!(
            form.contains("inset-0")
                && form.contains("h-dvh")
                && form.contains("aria-label=\"Close\"")
        );
        // The bare dialog has no frame and no close button of its own, but a hidden title.
        assert!(
            bare.contains("overflow-visible"),
            "popovers may leave the dialog box"
        );
        assert!(bare.contains("bg-transparent") && !bare.contains("aria-label=\"Close\""));
        assert!(bare.contains("class=\"sr-only\">T</h2>"));
        assert!(bare.contains("x-on:modal-close.window=\"$el.close()\""));
    }
}
