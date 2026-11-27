//! Form fields, validation errors and the menu with search.

use maud::{Markup, html};

use super::actions::{ButtonKind, ICON_BUTTON_CLASS, action_class};
use super::icons::{Icon, icon};
use super::tabs::{MarkerInset, TAB_CLASS, tab_marker};

/// Validation errors by field name; the empty name is a message for the whole form.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FormErrors(Vec<(std::borrow::Cow<'static, str>, String)>);

impl FormErrors {
    pub fn add(
        &mut self,
        field: impl Into<std::borrow::Cow<'static, str>>,
        message: impl Into<String>,
    ) {
        self.0.push((field.into(), message.into()));
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, field: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(f, _)| f == field)
            .map(|(_, m)| m.as_str())
    }

    /// Turns the collected errors into a result: `Ok(value)` if there are none.
    pub fn into_result<T>(self, value: T) -> Result<T, Self> {
        if self.is_empty() {
            Ok(value)
        } else {
            Err(self)
        }
    }

    /// Adds the errors of `other`.
    pub fn extend(&mut self, other: Self) {
        self.0.extend(other.0);
    }

    /// The fields with an error.
    pub fn fields(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(field, _)| field.as_ref())
    }
}

/// Classes of text inputs, selects and textareas; 16 px text so phones do not zoom on focus.
pub const INPUT_CLASS: &str = "min-h-11 w-full rounded-lg border border-line-strong bg-panel px-3 py-1.5 text-base text-ink placeholder:text-faint disabled:border-line disabled:bg-sunk disabled:text-faint lg:min-h-10";

/// Classes of a value that is shown but cannot be changed: muted on the `sunk` surface.
pub(crate) const READONLY_INPUT_CLASS: &str = "min-h-11 w-full rounded-lg border border-line bg-sunk px-3 py-1.5 font-mono text-base text-muted lg:min-h-10";

/// Classes of the text field of a [`MarkdownField`]: [`INPUT_CLASS`] without its minimum heights
/// (`min-h-11`, `lg:min-h-10`), which would beat the one `.md-input` sets from the field's rows
/// (`styles/input.css`), in monospace.
const MARKDOWN_INPUT_CLASS: &str = "md-input w-full rounded-lg border border-line-strong bg-panel px-3 py-1.5 font-mono text-base text-ink placeholder:text-faint disabled:border-line disabled:bg-sunk disabled:text-faint";

/// Classes of the compact read-only value of a dialog.
const READONLY_COMPACT_CLASS: &str =
    "h-9 w-full rounded-lg bg-sunk px-3 font-mono text-[13px] text-ink";

/// A labelled control with an optional hint and the field's error.
pub fn field(
    label: &str,
    name: &str,
    errors: &FormErrors,
    hint: Option<&str>,
    control: Markup,
) -> Markup {
    field_with_markup_hint(
        label,
        name,
        errors,
        hint.map(|hint| html! { (hint) }),
        control,
    )
}

/// [`field`] with a hint that is markup, for example one whose text Alpine switches.
pub fn field_with_markup_hint(
    label: &str,
    name: &str,
    errors: &FormErrors,
    hint: Option<Markup>,
    control: Markup,
) -> Markup {
    html! {
        div class="space-y-1" {
            label for=(name) class="block text-sm font-medium text-muted" { (label) }
            (control)
            @if let Some(hint) = hint { p id={(name) "-hint"} class="text-xs text-faint" { (hint) } }
            @if let Some(error) = errors.get(name) {
                p id={(name) "-error"} class="text-sm text-danger" role="alert" { (error) }
            }
        }
    }
}

/// The error message of a field or an input group, if any.
pub fn field_error(error: Option<&str>) -> Markup {
    html! {
        @if let Some(error) = error { p class="text-sm text-danger" role="alert" { (error) } }
    }
}

/// A single-line input.
pub fn text_field(
    label: &str,
    name: &str,
    value: &str,
    errors: &FormErrors,
    hint: Option<&str>,
) -> Markup {
    text_input(label, name, value, errors, hint, None, INPUT_CLASS)
}

/// A monospace single-line input that belongs to the form with the ID `form` (the `form`
/// attribute), for a field outside that form's element.
pub fn mono_text_field_of_form(
    label: &str,
    name: &str,
    value: &str,
    errors: &FormErrors,
    hint: Option<&str>,
    form: &str,
) -> Markup {
    let class = format!("{INPUT_CLASS} font-mono");
    text_input(label, name, value, errors, hint, Some(form), &class)
}

fn text_input(
    label: &str,
    name: &str,
    value: &str,
    errors: &FormErrors,
    hint: Option<&str>,
    form: Option<&str>,
    class: &str,
) -> Markup {
    field(
        label,
        name,
        errors,
        hint,
        html! {
            input id=(name) name=(name) type="text" value=(value) form=[form] class=(class)
                aria-invalid=[invalid(name, errors)] aria-describedby=[described_by(name, hint.is_some(), errors)];
        },
    )
}

/// A single-line value that cannot be changed (for example a project key after creation). It is
/// still submitted and carries its hint and error like any field.
pub fn readonly_field(
    label: &str,
    name: &str,
    value: &str,
    errors: &FormErrors,
    hint: Option<&str>,
) -> Markup {
    readonly(label, name, value, errors, hint, READONLY_INPUT_CLASS)
}

/// The compact [`readonly_field`] of a dialog: 36 px, borderless, the value in 13 px mono.
pub fn readonly_field_compact(
    label: &str,
    name: &str,
    value: &str,
    errors: &FormErrors,
    hint: Option<&str>,
) -> Markup {
    readonly(label, name, value, errors, hint, READONLY_COMPACT_CLASS)
}

fn readonly(
    label: &str,
    name: &str,
    value: &str,
    errors: &FormErrors,
    hint: Option<&str>,
    class: &str,
) -> Markup {
    field(
        label,
        name,
        errors,
        hint,
        html! {
            input id=(name) name=(name) type="text" value=(value) readonly class=(class)
                aria-invalid=[invalid(name, errors)] aria-describedby=[described_by(name, hint.is_some(), errors)];
        },
    )
}

/// A multi-line input.
pub fn textarea_field(
    label: &str,
    name: &str,
    value: &str,
    rows: u8,
    errors: &FormErrors,
) -> Markup {
    field(
        label,
        name,
        errors,
        None,
        html! {
            textarea id=(name) name=(name) rows=(rows) class=(INPUT_CLASS)
                aria-invalid=[invalid(name, errors)] aria-describedby=[described_by(name, false, errors)] { (value) }
        },
    )
}

/// Which views a [`MarkdownField`] offers next to its text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preview {
    /// "Editor" and "Preview" (modals and narrow places); opens on "Editor".
    Switch,
    /// "Editor", "Split" and "Preview" from 1280 px, "Editor" and "Preview" below; opens on
    /// "Split" where it is offered. The page holds the rendered preview of `value` (design D2a).
    Split,
    /// The label with the tabs "Write" and "Preview" at the right end of its row, no view switch,
    /// never the highlighting editor (`markdown-editor`, "Plain tabs layout"). Opens on "Write".
    PlainTabs,
}

/// A Markdown field (`markdown-editor` spec): a monospace text field with a view switch and the
/// server-rendered preview. The Alpine component `markdownField` (`scripts/markdownField.ts`)
/// runs it; without it the field is the plain labelled textarea.
#[derive(Debug, Clone, Copy)]
pub struct MarkdownField<'a> {
    pub label: &'a str,
    /// The base of the DOM ids and the key of the field's error; also the name of the form value
    /// unless `submit_name` is set.
    pub name: &'a str,
    /// The name of the form value when it differs from `name`: several fields of one page, such
    /// as the answers of the question cards of the inbox, need ids of their own.
    pub submit_name: Option<&'a str>,
    pub value: &'a str,
    pub hint: Option<&'a str>,
    /// The minimum height in text rows.
    pub rows: u8,
    pub preview: Preview,
    /// The label is for screen readers only: the field fills a page tab of that name.
    pub label_hidden: bool,
}

/// A [`MarkdownField`] with its hint and error.
pub fn markdown_field(field: &MarkdownField<'_>, errors: &FormErrors) -> Markup {
    let name = field.name;
    let split = field.preview == Preview::Split;
    let plain = field.preview == Preview::PlainTabs;
    // The marked field never attaches the highlighting editor.
    let options = if plain {
        serde_json::json!({ "plain": true })
    } else {
        serde_json::json!({ "split": split })
    };
    let label = html! {
        label for=(name) id={(name) "-label"} class=(if field.label_hidden { "sr-only" } else { "block text-sm font-medium text-muted" }) { (field.label) }
    };
    html! {
        div class="space-y-3" style={"--md-rows: " (field.rows)} x-data={"markdownField(" (options) ")"} {
            @if plain {
                div class="flex items-center justify-between gap-4" {
                    (label)
                    // Without the component Alpine cannot evaluate `enhanced`, so the tabs stay hidden.
                    div role="tablist" aria-label="View" x-cloak "x-show"="enhanced" "x-on:keydown"="move($event)"
                        class="flex gap-4" {
                        (plain_tab(name, "editor", "Write", true))
                        (plain_tab(name, "preview", "Preview", false))
                    }
                }
            } @else {
                (label)
                // Without the component Alpine cannot evaluate `enhanced`, so the row stays hidden.
                div class="relative flex min-h-8 items-center justify-center" x-cloak "x-show"="enhanced" {
                    div role="radiogroup" aria-label="View" "x-on:keydown"="move($event)"
                        class="inline-flex gap-0.5 rounded-lg border border-line-strong bg-panel p-0.5" {
                        (view_segment("editor", Icon::Pencil, "Editor", ""))
                        @if split { (view_segment("split", Icon::Columns, "Split", "max-xl:hidden")) }
                        (view_segment("preview", Icon::Eye, "Preview", ""))
                    }
                    // Only where the highlighting editor can attach (bundle embedded, from 768 px).
                    button type="button" role="switch" aria-checked="true" ":aria-checked"="highlighting"
                        x-cloak "x-show"="canEdit" "x-on:click"="toggle()"
                        class="absolute right-0 inline-flex min-h-8 items-center gap-2 text-[13px] font-medium text-muted" {
                        "Highlighting"
                        span aria-hidden="true" class="relative inline-block h-4 w-7 rounded-full" ":class"="highlighting ? 'bg-accent-text' : 'bg-muted'" {
                            span class="absolute top-0.5 size-3 rounded-full bg-panel" ":class"="highlighting ? 'left-3.5' : 'left-0.5'" {}
                        }
                    }
                }
            }
            div class="grid gap-4" ":class"="view === 'split' && 'md-split xl:grid-cols-2'" {
                // `:hidden`, not `x-show`: an expression Alpine cannot evaluate (the component did
                // not load) must leave the text field shown.
                div id={(name) "-write"} class="md-write min-w-0" ":hidden"="view === 'preview'"
                    role=[plain.then_some("tabpanel")] aria-labelledby=[plain.then(|| format!("{name}-tab-editor"))] {
                    textarea id=(name) name=(field.submit_name.unwrap_or(name)) rows=(field.rows)
                        class=(MARKDOWN_INPUT_CLASS)
                        aria-invalid=[invalid(name, errors)]
                        aria-describedby=[described_by(name, field.hint.is_some(), errors)] { (field.value) }
                }
                div id={(name) "-preview"} class="md-preview-panel min-w-0 rounded-lg border border-line bg-panel px-4 py-3.5 text-sm"
                    role=[plain.then_some("tabpanel")] aria-labelledby=[plain.then(|| format!("{name}-tab-preview"))]
                    aria-live="polite" x-cloak "x-show"="view !== 'editor'" {
                    // The status lines: above the preview and sticky, because in Split the panel
                    // scrolls and the last preview stays: a message at the top of the content
                    // would be out of view for a board member who scrolled down.
                    div data-preview-status class="sticky top-0 z-10 -mx-4 bg-panel px-4" {
                        p class="pb-2 pt-1 text-xs text-faint" x-cloak "x-show"="status === 'failed' && view === 'split'" { "Preview not up to date" }
                        p class="pb-2 pt-1 text-sm text-danger" x-cloak "x-show"="status === 'too-long'" { "Text too long for a preview (max 256 KiB)" }
                        p class="pb-2 pt-1 text-sm text-danger" x-cloak "x-show"="status === 'session'" { "Preview not available: your session has ended. Copy your text, then reload." }
                    }
                    div x-cloak "x-show"="status === 'ok' || view === 'split'" data-preview {
                        @if split { (crate::markdown_preview::fragment(field.value)) }
                    }
                    div x-cloak "x-show"="status === 'failed' && view !== 'split'" class="space-y-2" {
                        p class="text-sm text-danger" { "Preview not available" }
                        button type="button" "x-on:click"="retry()" class=(action_class(ButtonKind::Secondary)) { "Retry" }
                    }
                }
            }
            // While the highlighting editor is attached: Tab leaves it, so indenting has keys.
            @if !plain {
                p id={(name) "-indent"} data-indent-hint x-cloak "x-show"="editing" "x-text"="indentHint" class="text-xs text-faint" {
                    "Tab moves to the next field. Indent with Ctrl+] and Ctrl+[."
                }
            }
            @if let Some(hint) = field.hint { p id={(name) "-hint"} class="text-xs text-faint" { (hint) } }
            @if let Some(error) = errors.get(name) {
                p id={(name) "-error"} class="text-sm text-danger" role="alert" { (error) }
            }
        }
    }
}

/// One tab of the plain tabs layout: `view` is `editor` ("Write") or `preview`. The selected tab
/// is medium weight with a 2 px accent line, like the tabs of the settings pages.
fn plain_tab(name: &str, view: &str, label: &str, selected: bool) -> Markup {
    let panel = if view == "editor" { "write" } else { "preview" };
    html! {
        button type="button" role="tab" id={(name) "-tab-" (view)} data-view=(view)
            aria-controls={(name) "-" (panel)} aria-selected=(selected) tabindex=(if selected { "0" } else { "-1" })
            ":aria-selected"={"view === '" (view) "'"} ":tabindex"={"view === '" (view) "' ? 0 : -1"}
            "x-on:click"={"select('" (view) "')"}
            class={(TAB_CLASS) " px-1 text-[13px] lg:min-h-8"} {
            (tab_marker(MarkerInset::None))
            (label)
        }
    }
}

/// One segment of the view switch of a [`MarkdownField`].
fn view_segment(view: &str, glyph: Icon, label: &str, class: &str) -> Markup {
    html! {
        button type="button" role="radio" data-view=(view) aria-checked="false" tabindex="-1"
            ":aria-checked"={"view === '" (view) "'"} ":tabindex"={"view === '" (view) "' ? 0 : -1"}
            ":class"={"view === '" (view) "' ? 'bg-sunk font-medium text-ink' : 'text-muted'"}
            "x-on:click"={"select('" (view) "')"}
            class={"inline-flex min-h-7 items-center gap-1.5 rounded-md px-2.5 text-[13px] [&_svg]:size-3.5 " (class)} {
            (icon(glyph)) (label)
        }
    }
}

/// A select with an optional empty choice.
pub fn select_field(
    label: &str,
    name: &str,
    options: &[(String, String)],
    selected: &str,
    empty_label: Option<&str>,
    errors: &FormErrors,
) -> Markup {
    select_field_with_hint(label, name, options, selected, empty_label, None, errors)
}

/// A [`select_field`] with a hint under it.
pub fn select_field_with_hint(
    label: &str,
    name: &str,
    options: &[(String, String)],
    selected: &str,
    empty_label: Option<&str>,
    hint: Option<&str>,
    errors: &FormErrors,
) -> Markup {
    field(
        label,
        name,
        errors,
        hint,
        html! {
            select id=(name) name=(name) class=(INPUT_CLASS)
                aria-invalid=[invalid(name, errors)] aria-describedby=[described_by(name, hint.is_some(), errors)] {
                @if let Some(empty) = empty_label { option value="" selected[selected.is_empty()] { (empty) } }
                @for (value, text) in options {
                    option value=(value) selected[value == selected] { (text) }
                }
            }
        },
    )
}

/// A form-wide error message, if any.
pub fn form_error(errors: &FormErrors) -> Markup {
    html! {
        @if let Some(error) = errors.get("") {
            p class="rounded-lg bg-danger-soft px-3 py-2 text-sm text-danger" role="alert" { (error) }
        }
    }
}

/// One entry of a [`search_menu`].
#[derive(Debug, Clone)]
pub struct MenuItem {
    pub href: String,
    pub label: String,
    /// Secondary text; also searched. For example a key or a count.
    pub hint: String,
    pub current: bool,
}

/// A button that opens a menu of links with a search field (Alpine). The search filters the
/// rendered entries by label and hint in the browser.
pub fn search_menu(button_label: &str, items: &[MenuItem]) -> Markup {
    html! {
        div class="relative" x-data="{ open: false, q: '' }" "x-on:keydown.escape"="open = false" "x-on:click.outside"="open = false" {
            button type="button" aria-haspopup="true" ":aria-expanded"="open" "x-on:click"="open = !open"
                class="inline-flex min-h-11 items-center gap-1 rounded-[10px] border border-line bg-panel px-3 text-sm text-muted hover:bg-sunk lg:min-h-8" {
                (button_label) (icon(Icon::ChevronDown))
            }
            div x-show="open" x-cloak
                class="absolute left-0 z-20 mt-1 w-64 rounded-xl border border-line bg-panel p-2 shadow-lg" {
                label class="sr-only" for="menu-search" { "Search" }
                input id="menu-search" type="search" placeholder="Search" x-model="q" "x-effect"="open && $nextTick(() => $el.focus())"
                    class=(INPUT_CLASS);
                ul class="mt-2 max-h-64 overflow-y-auto" {
                    @for item in items {
                        li data-search=(format!("{} {}", item.label, item.hint).to_lowercase())
                            "x-show"="$el.dataset.search.includes(q.trim().toLowerCase())" {
                            a href=(item.href) aria-current=[item.current.then_some("true")]
                                class={"flex min-h-11 items-center justify-between gap-2 rounded-lg px-2 text-sm hover:bg-sunk lg:min-h-9 " (if item.current { "text-accent-text font-medium" } else { "text-ink" })} {
                                span class="truncate" { (item.label) }
                                span class="font-mono text-xs text-faint" { (item.hint) }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// An icon button that opens a menu of radio options (links); the one marked `current` has a
/// check. `label` names the current option: it is the button's accessible name and tooltip.
pub fn icon_menu(glyph: Icon, label: &str, items: &[MenuItem]) -> Markup {
    html! {
        div class="relative" x-data="{ open: false }" "x-on:keydown.escape"="open = false" "x-on:click.outside"="open = false" {
            button type="button" aria-haspopup="menu" ":aria-expanded"="open" "x-on:click"="open = !open"
                aria-label=(label) title=(label) class=(ICON_BUTTON_CLASS) {
                (icon(glyph))
            }
            div x-show="open" x-cloak role="menu" aria-label=(label)
                class="absolute right-0 z-20 mt-1 w-56 rounded-xl border border-line bg-panel p-2 shadow-lg md:left-0 md:right-auto" {
                @for item in items {
                    a href=(item.href) role="menuitemradio" aria-checked=(item.current)
                        class={"flex min-h-11 items-center justify-between gap-2 rounded-lg px-2 text-sm hover:bg-sunk lg:min-h-9 " (if item.current { "font-medium text-ink" } else { "text-muted" })} {
                        (item.label)
                        @if item.current { span class="text-accent-text" { (icon(Icon::Check)) } }
                    }
                }
            }
        }
    }
}

/// A titled group of fields of a tab or form: a region named by its heading, separated from the
/// group above by a line (none above the first group of its parent). The heading id is derived
/// from `title`, so a page holds each title once.
pub fn form_group(title: &str, body: Markup) -> Markup {
    let heading_id = format!(
        "group-{}",
        title
            .to_lowercase()
            .replace(|c: char| !c.is_alphanumeric(), "-")
    );
    html! {
        section aria-labelledby=(heading_id) class="space-y-4 border-t border-line pt-5 first:border-t-0 first:pt-0" {
            h3 id=(heading_id) class="text-[15px] font-semibold" { (title) }
            (body)
        }
    }
}

/// `aria-invalid` of a control: set while the field has an error.
pub(crate) fn invalid(name: &str, errors: &FormErrors) -> Option<&'static str> {
    errors.get(name).map(|_| "true")
}

/// `aria-describedby` of a control: the ids of its hint and error that [`field`] renders.
pub(crate) fn described_by(name: &str, has_hint: bool, errors: &FormErrors) -> Option<String> {
    let ids: Vec<String> = [
        has_hint.then(|| format!("{name}-hint")),
        errors.get(name).map(|_| format!("{name}-error")),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!ids.is_empty()).then(|| ids.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::tag_around;

    #[test]
    fn input_classes_pad_so_the_desktop_height_stays_40px() {
        // 6 + 6 padding + 24 line + 2 border = 38 px, so `lg:min-h-10` sets the height.
        for class in [INPUT_CLASS, READONLY_INPUT_CLASS] {
            let classes: Vec<_> = class.split(' ').collect();
            assert!(classes.contains(&"py-1.5"), "{class}");
            assert!(!classes.contains(&"py-2"), "{class}");
        }
    }

    #[test]
    fn field_shows_label_hint_and_error() {
        let mut errors = FormErrors::default();
        errors.add("name", "Name is required.");
        let html = text_field("Name", "name", "x<y", &errors, Some("Short.")).into_string();
        assert!(html.contains("<label for=\"name\""), "{html}");
        assert!(html.contains("value=\"x&lt;y\"") && html.contains("Short."));
        assert!(html.contains("role=\"alert\"") && html.contains("Name is required."));
        assert!(html.contains("min-h-11"));
    }

    #[test]
    fn an_icon_menu_opens_leftwards_on_phones_and_from_the_button_edge_from_768_px() {
        let item = MenuItem {
            href: "/x".into(),
            label: "A".into(),
            hint: String::new(),
            current: true,
        };
        let html = icon_menu(Icon::ArrowDownUp, "A", &[item]).into_string();
        let panel = &html[html.find("role=\"menu\"").unwrap()..];
        assert!(
            panel.contains("right-0") && panel.contains("md:left-0 md:right-auto"),
            "{panel}"
        );
    }

    #[test]
    fn a_form_group_is_a_region_named_by_its_heading_with_a_separator_below_the_first() {
        let html = form_group("Forge events", html! { p { "body" } }).into_string();
        assert!(
            html.starts_with("<section aria-labelledby=\"group-forge-events\""),
            "{html}"
        );
        assert!(
            html.contains("<h3 id=\"group-forge-events\" class=\"text-[15px] font-semibold\">Forge events</h3>"),
            "{html}"
        );
        assert!(html.contains("<p>body</p>"), "{html}");
        // The separator is a top border, removed on the first child of the parent.
        let classes = html.split("class=\"").nth(1).unwrap();
        for class in ["border-t", "border-line", "first:border-t-0", "first:pt-0"] {
            assert!(
                classes.split(' ').any(|c| c.starts_with(class)),
                "{class}: {html}"
            );
        }
    }

    #[test]
    fn a_readonly_field_is_muted_submitted_and_linked_to_its_error() {
        let mut errors = FormErrors::default();
        errors.add("key", "Fixed.");
        let html =
            readonly_field("Key", "key", "DF", &errors, Some("Never changes.")).into_string();
        assert!(
            html.contains(" readonly")
                && html.contains("name=\"key\"")
                && html.contains("value=\"DF\""),
            "{html}"
        );
        assert!(html.contains("bg-sunk") && html.contains("text-muted"));
        assert!(
            html.contains("aria-invalid=\"true\"")
                && html.contains("aria-describedby=\"key-hint key-error\"")
        );
    }

    #[test]
    fn a_compact_readonly_field_is_a_borderless_mono_value_linked_to_its_error() {
        let mut errors = FormErrors::default();
        errors.add("destination", "Invalid.");
        let html = readonly_field_compact("Destination", "destination", "a.b:443", &errors, None)
            .into_string();
        let input = &html[html.find("<input").unwrap()..];
        let input = &input[..input.find('>').unwrap()];
        assert!(
            input.contains(" readonly")
                && input.contains("name=\"destination\"")
                && input.contains("value=\"a.b:443\""),
            "{input}"
        );
        assert!(
            input.contains("h-9")
                && input.contains("bg-sunk")
                && input.contains("font-mono text-[13px]")
                && !input.contains("border"),
            "{input}"
        );
        assert!(
            input.contains("aria-invalid=\"true\"")
                && input.contains("aria-describedby=\"destination-error\""),
            "{input}"
        );
    }

    #[test]
    fn controls_reference_their_hint_and_error() {
        let mut errors = FormErrors::default();
        errors.add("name", "Name is required.");
        let html = text_field("Name", "name", "", &errors, Some("Short.")).into_string();
        assert!(html.contains("aria-invalid=\"true\""), "{html}");
        assert!(
            html.contains("aria-describedby=\"name-hint name-error\""),
            "{html}"
        );
        assert!(html.contains("id=\"name-hint\"") && html.contains("id=\"name-error\""));

        let clean = text_field("Name", "name", "", &FormErrors::default(), None).into_string();
        assert!(!clean.contains("aria-invalid") && !clean.contains("aria-describedby"));

        let options = vec![("a".to_string(), "A".to_string())];
        let mut errors = FormErrors::default();
        errors.add("n", "Pick one.");
        let select = select_field("L", "n", &options, "a", None, &errors).into_string();
        assert!(
            select.contains("aria-describedby=\"n-error\"")
                && select.contains("aria-invalid=\"true\"")
        );
        let area = textarea_field("P", "p", "", 3, &errors).into_string();
        assert!(
            !area.contains("aria-invalid"),
            "error of another field: {area}"
        );
    }

    fn markdown(preview: Preview, label_hidden: bool, errors: &FormErrors) -> String {
        markdown_field(
            &MarkdownField {
                label: "System prompt",
                name: "system_prompt",
                submit_name: None,
                value: "## Rules\n\n- one <b>",
                hint: Some("Markdown."),
                rows: 20,
                preview,
                label_hidden,
            },
            errors,
        )
        .into_string()
    }

    #[test]
    fn a_markdown_field_is_a_labelled_monospace_textarea_like_textarea_field() {
        let mut errors = FormErrors::default();
        errors.add("system_prompt", "Too long.");
        let html = markdown(Preview::Switch, false, &errors);
        assert!(
            html.contains("<label for=\"system_prompt\" id=\"system_prompt-label\" class=\"block text-sm font-medium text-muted\">System prompt</label>"),
            "{html}"
        );
        let area = &html[html.find("<textarea").unwrap()..];
        let area = &area[..area.find("</textarea>").unwrap()];
        for part in [
            "id=\"system_prompt\"",
            "name=\"system_prompt\"",
            "rows=\"20\"",
            "font-mono",
            "md-input",
            "aria-invalid=\"true\"",
            "aria-describedby=\"system_prompt-hint system_prompt-error\"",
            ">## Rules\n\n- one &lt;b&gt;",
        ] {
            assert!(area.contains(part), "{part}: {area}");
        }
        assert!(html.contains("id=\"system_prompt-hint\"") && html.contains("Too long."));
        assert!(html.contains("--md-rows: 20"), "{html}");
    }

    #[test]
    fn the_text_field_has_no_minimum_height_utility_so_its_rows_decide() {
        // `min-h-11` of `INPUT_CLASS` (the utilities layer) would beat `.md-input`'s minimum
        // height from `--md-rows` (the components layer): the field would be 44 px, not 20 rows.
        let html = markdown(Preview::Switch, false, &FormErrors::default());
        let area = tag_around(&html, "rows=\"20\"");
        let class = area
            .split("class=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let classes: Vec<_> = class.split(' ').collect();
        assert!(classes.contains(&"md-input"), "{class}");
        assert!(classes.iter().all(|c| !c.contains("min-h-")), "{class}");
    }

    #[test]
    fn the_markdown_input_class_is_the_input_class_without_its_minimum_heights_in_monospace() {
        let markdown: std::collections::BTreeSet<_> = MARKDOWN_INPUT_CLASS.split(' ').collect();
        let input: std::collections::BTreeSet<_> = INPUT_CLASS
            .split(' ')
            .filter(|class| !class.contains("min-h-"))
            .chain(["md-input", "font-mono"])
            .collect();
        assert_eq!(markdown, input);
    }

    #[test]
    fn a_hidden_label_is_for_screen_readers_only() {
        let html = markdown(Preview::Split, true, &FormErrors::default());
        assert!(
            html.contains("<label for=\"system_prompt\" id=\"system_prompt-label\" class=\"sr-only\">System prompt</label>"),
            "{html}"
        );
    }

    #[test]
    fn the_switch_view_has_editor_and_preview_in_a_radio_group_hidden_until_alpine_runs() {
        let html = markdown(Preview::Switch, false, &FormErrors::default());
        let switch = &html[html.find("role=\"radiogroup\"").unwrap()..];
        let switch = &switch[..switch.find("</div>").unwrap()];
        assert_eq!(switch.matches("role=\"radio\"").count(), 2, "{switch}");
        assert!(switch.contains("Editor") && switch.contains("Preview"));
        assert!(!switch.contains("Split"), "{switch}");
        let row = &html[..html.find("role=\"radiogroup\"").unwrap()];
        assert!(
            row.rsplit("<div").nth(1).unwrap().contains("x-cloak"),
            "{row}"
        );
        assert!(html.contains("aria-live=\"polite\""), "{html}");
        assert!(!html.contains("role=\"tab\""), "{html}");
        // Switch renders no preview before the first request.
        assert!(!html.contains("<h2>Rules</h2>"), "{html}");
    }

    #[test]
    fn the_split_view_adds_split_from_1280_px_and_renders_the_first_preview() {
        let html = markdown(Preview::Split, false, &FormErrors::default());
        assert!(
            html.contains("x-data=\"markdownField({&quot;split&quot;:true})\""),
            "{html}"
        );
        let split = &html[html.find("data-view=\"split\"").unwrap()..];
        let split = &split[..split.find("</button>").unwrap()];
        assert!(
            split.contains("max-xl:hidden") && split.contains("Split"),
            "{split}"
        );
        assert_eq!(html.matches("role=\"radio\"").count(), 3);
        assert!(html.contains("xl:grid-cols-2"), "{html}");
        let preview = &html[html.find("id=\"system_prompt-preview\"").unwrap()..];
        assert!(preview.contains("<h2>Rules</h2>"), "{preview}");
        assert!(preview.contains("&lt;b&gt;"), "{preview}");
        assert!(!html.contains("role=\"tab\""), "{html}");
    }

    #[test]
    fn the_highlighting_switch_and_the_indent_hint_wait_for_the_script() {
        for preview in [Preview::Switch, Preview::Split] {
            let html = markdown(preview, false, &FormErrors::default());
            // The switch: at the right end of the row of the view switch, hidden until an editor
            // can attach (`canEdit`), on by default.
            let switch = tag_around(&html, "role=\"switch\"");
            assert!(
                switch.contains("x-show=\"canEdit\"")
                    && switch.contains("x-cloak")
                    && switch.contains(":aria-checked=\"highlighting\"")
                    && switch.contains("absolute right-0")
                    && switch.contains("x-on:click=\"toggle()\""),
                "{switch}"
            );
            assert!(html.contains(">Highlighting<"), "{html}");
            // The hint below the field names the keys and shows only while an editor is attached.
            let hint = tag_around(&html, "data-indent-hint");
            assert!(
                hint.contains("id=\"system_prompt-indent\"")
                    && hint.contains("x-show=\"editing\"")
                    && hint.contains("x-cloak"),
                "{hint}"
            );
            assert!(
                html.contains("Tab moves to the next field. Indent with Ctrl+] and Ctrl+[."),
                "{html}"
            );
            let (switch_at, hint_at) = (
                html.find("role=\"switch\"").unwrap(),
                html.find("data-indent-hint").unwrap(),
            );
            assert!(
                html.find("role=\"radiogroup\"").unwrap() < switch_at
                    && html.find("<textarea").unwrap() < hint_at,
                "{html}"
            );
        }
    }

    #[test]
    fn plain_tabs_put_the_label_and_the_write_and_preview_tabs_in_one_row() {
        let html = markdown(Preview::PlainTabs, false, &FormErrors::default());

        let row_at = html.find("role=\"tablist\"").unwrap();
        let (label_at, row) = (html.find("<label").unwrap(), &html[..row_at]);
        assert!(label_at < row_at, "{html}");
        // The label and the tablist share one flex row, the label on the left.
        assert!(
            row.rsplit("<div")
                .nth(1)
                .unwrap()
                .contains("justify-between"),
            "{row}"
        );
        let tabs = &html[row_at..html.find("</div>").unwrap()];
        assert_eq!(tabs.matches("role=\"tab\"").count(), 2, "{tabs}");
        assert!(
            tabs.contains(">Write<") && tabs.contains(">Preview<"),
            "{tabs}"
        );
        let write = tag_around(tabs, "data-view=\"editor\"");
        assert!(
            write.contains("aria-selected=\"true\"") && write.contains("tabindex=\"0\""),
            "{write}"
        );
        let preview = tag_around(tabs, "data-view=\"preview\"");
        assert!(
            preview.contains("aria-selected=\"false\"") && preview.contains("tabindex=\"-1\""),
            "{preview}"
        );
        assert!(
            write.contains("aria-controls=\"system_prompt-write\"")
                && preview.contains("aria-controls=\"system_prompt-preview\""),
            "{write} {preview}"
        );
        assert!(html.contains("x-on:keydown=\"move($event)\""), "{html}");
    }

    #[test]
    fn plain_tabs_have_no_view_switch_no_highlighting_and_no_editor() {
        let html = markdown(Preview::PlainTabs, false, &FormErrors::default());

        for absent in [
            "role=\"radiogroup\"",
            "role=\"radio\"",
            "role=\"switch\"",
            "Highlighting",
            "data-indent-hint",
            "Indent with",
            "Split",
            "editor.js",
            "data-editor-src",
        ] {
            assert!(!html.contains(absent), "{absent}: {html}");
        }
        // The field is marked so the component never attaches the highlighting editor.
        assert!(
            html.contains("x-data=\"markdownField({&quot;plain&quot;:true})\""),
            "{html}"
        );
        // The text is 16 px, so the browser does not zoom on focus.
        assert!(
            tag_around(&html, "rows=\"20\"").contains("text-base"),
            "{html}"
        );
    }

    #[test]
    fn plain_tabs_without_the_component_are_the_labelled_text_field() {
        let html = markdown(Preview::PlainTabs, false, &FormErrors::default());

        let tablist = tag_around(&html, "role=\"tablist\"");
        assert!(
            tablist.contains("x-cloak") && tablist.contains("x-show=\"enhanced\""),
            "{tablist}"
        );
        let write = tag_around(&html, "id=\"system_prompt-write\"");
        assert!(
            write.contains(":hidden=\"view === 'preview'\"") && !write.contains("x-show"),
            "{write}"
        );
        assert!(html.contains("<label for=\"system_prompt\""), "{html}");
        assert!(
            !html.contains("<h2>Rules</h2>"),
            "no preview before the first request: {html}"
        );
    }

    #[test]
    fn the_text_field_stays_shown_and_the_switch_hidden_when_the_component_does_not_run() {
        let html = markdown(Preview::Split, false, &FormErrors::default());
        // An `x-show` that Alpine cannot evaluate hides its element: the panel of the text field
        // therefore hides itself through `:hidden`, which an unevaluated binding leaves unset.
        let write = tag_around(&html, "id=\"system_prompt-write\"");
        assert!(
            write.contains(":hidden=\"view === 'preview'\"") && !write.contains("x-show"),
            "{write}"
        );
        // The switch row waits for the component's `enhanced`, so it stays hidden without it.
        let row = tag_around(&html, "class=\"relative flex min-h-8");
        assert!(
            row.contains("x-cloak") && row.contains("x-show=\"enhanced\""),
            "{row}"
        );
    }

    #[test]
    fn the_last_preview_of_the_split_view_stays_for_every_failure_with_its_message() {
        let html = markdown(Preview::Split, false, &FormErrors::default());
        let content = tag_around(&html, "data-preview>");
        assert!(
            content.contains("x-show=\"status === 'ok' || view === 'split'\""),
            "{content}"
        );
        for (status, text) in [
            ("failed", "Preview not up to date"),
            ("too-long", "Text too long for a preview (max 256 KiB)"),
            ("session", "your session has ended"),
        ] {
            let at = html.find(text).unwrap_or_else(|| panic!("{text}"));
            let tag = &html[html[..at].rfind('<').unwrap()..at];
            assert!(tag.contains(&format!("status === '{status}'")), "{tag}");
            assert!(!tag.contains("view !== 'split'"), "{tag}");
            // In the status block, which sits above the preview and is sticky: a board member
            // who scrolled a long preview down still sees the message.
            let status = tag_around(&html, "data-preview-status");
            assert!(status.contains("sticky top-0"), "{status}");
            let (block, preview) = (
                html.find("data-preview-status").unwrap(),
                html.find("data-preview>").unwrap(),
            );
            assert!(
                block < at && at < preview,
                "{text} is not in the status block"
            );
        }
    }

    #[test]
    fn the_preview_states_are_messages_of_the_field() {
        let html = markdown(Preview::Split, false, &FormErrors::default());
        for text in [
            "Preview not up to date",
            "Preview not available",
            "Retry",
            "Text too long for a preview (max 256 KiB)",
            "Preview not available: your session has ended. Copy your text, then reload.",
        ] {
            assert!(html.contains(text), "{text}: {html}");
        }
    }

    #[test]
    fn select_marks_the_chosen_option() {
        let options = vec![
            ("a".to_string(), "A".to_string()),
            ("b".to_string(), "B".to_string()),
        ];
        let html =
            select_field("L", "n", &options, "b", None, &FormErrors::default()).into_string();
        assert!(html.contains("<option value=\"b\" selected>"), "{html}");
        assert!(!html.contains("<option value=\"a\" selected>"));
    }

    #[test]
    fn the_menu_trigger_is_a_rounded_rectangle() {
        let html = search_menu("All projects", &[]).into_string();
        let trigger = &html[html.find("<button").unwrap()..];
        let trigger = &trigger[..trigger.find('>').unwrap()];
        assert!(
            trigger.contains("rounded-[10px]") && !trigger.contains("rounded-full"),
            "{trigger}"
        );
    }

    #[test]
    fn menu_lists_every_entry_with_search_data() {
        let items = vec![
            MenuItem {
                href: "/tasks?project=DF".into(),
                label: "Dark Factory".into(),
                hint: "DF".into(),
                current: true,
            },
            MenuItem {
                href: "/tasks?project=WEB".into(),
                label: "Website".into(),
                hint: "WEB".into(),
                current: false,
            },
        ];
        let html = search_menu("All projects", &items).into_string();
        assert!(
            html.contains("data-search=\"dark factory df\"")
                && html.contains("data-search=\"website web\""),
            "{html}"
        );
        assert!(html.contains("x-model=\"q\"") && html.contains("aria-current=\"true\""));
    }

    #[test]
    fn form_error_renders_only_the_form_wide_message() {
        let mut errors = FormErrors::default();
        errors.add("", "Broken.");
        assert!(form_error(&errors).into_string().contains("Broken."));
        assert!(form_error(&FormErrors::default()).into_string().is_empty());
    }
}
