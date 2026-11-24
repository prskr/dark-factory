//! Markup of the chat workspace: the project and chat list, the header, the details and the new
//! chat dialog (`specs/chat-ui/spec.md`). The live-update regions (`#chat-head`, `#chat-overview`,
//! `#chat-rows`, and the shared `#runs`, `#pending`, ...) are re-rendered by `crate::sse`.

use df_core::{TITLE_MAX_CHARS, TaskStatus, TitleSource};
use df_db::catalog::PersonaRecord;
use df_db::chat_views::{ChatListItem, ChatProject};
use df_db::task_views::{RunView, SubtaskItem, TaskDetail};
use maud::{Markup, html};

use crate::artifacts::{ArtifactCard, chat_details_heading};
use crate::components::forms::{INPUT_CLASS, MenuItem};
use crate::components::short_time;
use crate::components::{
    ButtonKind, DialogStyle, EmptyPanel, EmptySize, FormErrors, ICON_BUTTON_CLASS, Icon,
    ROUND_ACTION_CLASS, Tone, action_class, avatar, definition_list, details_panel_with_heading,
    dialog_close, dialog_with, field_error, form_error, harness_label, icon, initials,
    phone_profile_menu, search_menu, section, status_badge,
};
use crate::layout::Shell;
use crate::task_pages::{runs_region, status_name};

/// Why a chat can't be archived right now; `None` when it can.
pub(super) fn archive_blocker(detail: &TaskDetail) -> Option<String> {
    if detail.archived_at.is_some() {
        return None;
    }
    let reason =
        df_core::task_transition::archive_rejection(detail.status, detail.has_pending_request)?
            .to_string();
    let mut chars = reason.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

fn url_encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// The dot of a chat that needs a board member: an open approval request.
fn needs_you_dot() -> Markup {
    html! {
        span role="img" aria-label="Needs you" title="Needs you"
            class={"inline-block size-2.5 shrink-0 rounded-full " (Tone::Limited.dot_class())} {}
    }
}

/// The chat list: the chats of the selected project and the entry of the archive, replaced as a
/// whole when a chat changes. A chat that needs you carries the dot instead of a badge, an idle
/// chat none at all.
pub(crate) fn chat_rows(
    chats: &[ChatListItem],
    project: &ChatProject,
    current: Option<&str>,
) -> Markup {
    html! {
        div id="chat-rows" {
            ul {
                @for chat in chats {
                    @let href = format!("/chats/{}", chat.key);
                    li id={"row-" (chat.key)} {
                        a href=(href) hx-get=(href) hx-target="#ws-detail" hx-select="#ws-detail" hx-swap="outerHTML" hx-push-url="true"
                            aria-current=[(current == Some(chat.key.as_str())).then_some("page")]
                            "x-on:click"="select($el)"
                            class="block min-h-11 border-b border-line border-l-[3px] border-l-transparent py-3.5 pr-5 pl-[17px] hover:bg-sunk aria-[current=page]:border-l-accent aria-[current=page]:bg-accent-soft" {
                            div class="flex items-center gap-2" {
                                (avatar(&initials(&chat.persona_display_name)))
                                span class="min-w-0 truncate text-sm font-medium" { (chat.persona_display_name) }
                                @if chat.needs_you { (needs_you_dot()) }
                                @else if chat.status != TaskStatus::Waiting { (status_badge(chat.status)) }
                                time datetime=(chat.last_activity_at.to_string()) class="ml-auto shrink-0 font-mono text-xs text-faint" { (short_time(chat.last_activity_at)) }
                            }
                            div title=(chat.title) class={"mt-1 truncate text-[15px] font-medium" @if chat.title_source == TitleSource::Placeholder { " italic text-muted" }} { (chat.title) }
                            @if chat.title_source == TitleSource::Placeholder {
                                div class="mt-0.5 truncate text-xs text-muted" { (chat.persona_display_name) " sets the topic when the run starts" }
                            } @else if !chat.last_message.is_empty() {
                                div class="mt-0.5 truncate text-xs text-muted" { (chat.last_message) }
                            }
                        }
                    }
                }
            }
            button type="button" hx-get={"/chats/archive?project=" (url_encode(&project.key))} hx-target="#composer-slot" hx-swap="innerHTML"
                class="flex min-h-11 w-full items-center gap-2 border-b border-line px-5 text-left text-sm text-muted hover:bg-sunk" {
                (icon(Icon::Archive)) "Archived chats"
                span class="ml-auto rounded-full bg-sunk px-1.5 font-mono text-xs" { (project.archived) }
            }
        }
    }
}

/// The projects with their number of chats, the selected project's chats and, collapsed, one
/// avatar per persona with a chat.
pub(super) fn list_column(
    projects: &[ChatProject],
    selected: &ChatProject,
    chats: &[ChatListItem],
    current: Option<&str>,
    shell: &Shell,
) -> Markup {
    // The chat of a persona that needs you, else its most recently active one: the list is in that
    // order, so the first chat of each persona is it.
    let mut personas: Vec<&ChatListItem> = Vec::new();
    for chat in chats {
        if !personas.iter().any(|p| p.persona_id == chat.persona_id) {
            personas.push(chat);
        }
    }
    let phone_menu: Vec<MenuItem> = projects
        .iter()
        .map(|p| MenuItem {
            href: format!("/chats?project={}", url_encode(&p.key)),
            label: p.name.clone(),
            hint: format!("{} · {}", p.key, p.chats),
            current: p.key == selected.key,
        })
        .collect();
    html! {
        section class="ws-list" aria-label="Chats" {
            // From 768 px the list collapses to this strip (`listCollapsed` of the layout).
            div class="ws-list-strip flex-col items-center gap-2 py-3" {
                button type="button" aria-label="Expand chat list" "x-on:click"="setListCollapsed(false)" class=(ICON_BUTTON_CLASS) { (icon(Icon::PanelLeft)) }
                @for chat in &personas {
                    a href={"/chats/" (chat.key)} aria-label=(format!("Chat with {}", chat.persona_display_name)) title=(chat.persona_display_name) class="relative rounded-full" {
                        (avatar(&initials(&chat.persona_display_name)))
                        @if chat.needs_you { span class="absolute -right-0.5 -top-0.5" { (needs_you_dot()) } }
                    }
                }
            }
            div class="ws-list-full space-y-3 border-b border-line p-4" {
                div class="flex items-center justify-between gap-2" {
                    h1 class="text-lg font-semibold" { "Chats" }
                    div class="ml-auto md:hidden" { (search_menu(&selected.name, &phone_menu)) }
                    button type="button" aria-label="Collapse chat list" "x-on:click"="setListCollapsed(true)" class={(ICON_BUTTON_CLASS) " ml-auto max-md:hidden"} { (icon(Icon::PanelLeft)) }
                    (phone_profile_menu(shell))
                }
                nav aria-label="Projects" class="hidden md:block" {
                    ul class="-mx-2" {
                        @for project in projects {
                            @let current = project.key == selected.key;
                            li {
                                a href={"/chats?project=" (url_encode(&project.key))} aria-current=[current.then_some("true")]
                                    class={"flex min-h-9 items-center gap-2 rounded-lg px-2 text-sm hover:bg-sunk " (if current { "bg-sunk font-medium text-ink" } else { "text-muted" })} {
                                    span class="min-w-0 flex-1 truncate" { (project.name) }
                                    @if project.needs_you { (needs_you_dot()) }
                                    span class="font-mono text-xs text-faint" { (project.chats) }
                                }
                            }
                        }
                    }
                }
            }
            div class="ws-list-full min-h-0 flex-1 overflow-y-auto pb-22 md:pb-0" {
                div hx-sse:connect={"/chats/events?project=" (url_encode(&selected.key))} hx-swap="none" {}
                (chat_rows(chats, selected, current))
            }
        }
    }
}

/// The center of a project without a selected chat: the empty state of a project without chats,
/// or the hint to pick one.
pub(super) fn center_empty(project: &ChatProject) -> Markup {
    let compose = format!("/chats/compose?project={}", url_encode(&project.key));
    let (title, text) = if project.chats == 0 {
        (
            format!("No chats in {} yet", project.name),
            "A chat is a conversation with a persona. Ask questions about the code, plan work or let it hand work to tasks.",
        )
    } else {
        (
            "No chat selected".to_owned(),
            "Pick a chat from the list, or start a new one with a persona.",
        )
    };
    html! {
        section class="ws-main items-center justify-center" aria-label="Chat" {
            (EmptyPanel {
                size: EmptySize::Workspace,
                icon: Some(icon(Icon::Chat)),
                title: &title,
                text,
                phone_hint: None,
                action: html! {
                    button type="button" hx-get=(compose) hx-target="#composer-slot" hx-swap="innerHTML"
                        class={(action_class(ButtonKind::Primary)) " mt-1.5"} { (icon(Icon::Plus)) "New chat" }
                },
            }.render())
        }
    }
}

/// The header of the center: the action "New chat" (desktop), and on phones the way back to the
/// list, the persona, the topic and the status.
pub(crate) fn head_region(task: &TaskDetail) -> Markup {
    let compose = format!("/chats/compose?project={}", url_encode(&task.project_key));
    let list = format!("/chats?project={}", url_encode(&task.project_key));
    html! {
        header id="chat-head" class="border-b border-line bg-panel px-2 py-2 lg:px-4" {
            div class="flex items-center gap-1" {
                a href=(list) aria-label="Back to chats" class={(ICON_BUTTON_CLASS) " md:hidden"} { (icon(Icon::ChevronLeft)) }
                div class="flex min-w-0 flex-1 items-center gap-2 md:hidden" {
                    (avatar(&initials(&task.persona_display_name)))
                    div class="min-w-0" {
                        div class="truncate text-sm font-semibold" title=(task.title) { (task.title) }
                        div class="truncate text-xs text-muted" { (task.persona_display_name) }
                    }
                    @if task.status != TaskStatus::Waiting { (status_badge(task.status)) }
                }
                // From 768 px the persona block is hidden and nothing grows: "New chat" alone takes
                // the free space, so "Details" stays at the right edge (a second `ml-auto` would
                // split it and put "Details" mid-header).
                button type="button" hx-get=(compose) hx-target="#composer-slot" hx-swap="innerHTML"
                    class={(action_class(ButtonKind::Primary)) " ml-auto max-md:hidden"} { (icon(Icon::Plus)) "New chat" }
                button type="button" aria-label="Details" "x-on:click"="details = true; sheet = 'half'" class={(ICON_BUTTON_CLASS) " xl:hidden"} { (icon(Icon::PanelRight)) }
            }
        }
    }
}

/// What replaces the message box of an archived chat.
pub(crate) fn archived_notice(key: &str) -> Markup {
    html! {
        div id="archived-notice" class="flex items-center justify-between gap-3 border-t border-line bg-panel p-3" {
            p class="text-sm text-muted" { "This chat is archived. Unarchive it to write to it." }
            button type="button" hx-post={"/chats/" (key) "/unarchive"} hx-swap="none"
                class=(action_class(ButtonKind::Secondary)) { "Unarchive" }
        }
    }
}

/// Agent, now, status with the archive action, topic, project, persona, key and checkout; updated
/// live. It also renders the strip the details collapse to.
pub(crate) fn overview_region(task: &TaskDetail, base_branch: &str, error: Option<&str>) -> Markup {
    let blocker = archive_blocker(task);
    let rename = format!("/chats/{}/title", task.key);
    let status_name = status_name(task.status);
    let properties: Vec<(&str, Markup)> = vec![
        (
            "Status",
            html! {
                div class="flex flex-wrap items-center gap-2" {
                    (status_badge(task.status))
                    @if task.archived_at.is_some() {
                        span class="text-xs text-muted" { "archived" }
                        button type="button" hx-post={"/chats/" (task.key) "/unarchive"} hx-swap="none"
                            class=(action_class(ButtonKind::Secondary)) { "Unarchive" }
                    } @else {
                        button type="button" hx-post={"/chats/" (task.key) "/archive"} hx-target="#chat-overview" hx-swap="outerMorph"
                            disabled[blocker.is_some()] title=[blocker.as_deref()]
                            class=(action_class(ButtonKind::Secondary)) { (icon(Icon::Archive)) "Archive" }
                    }
                }
                @if let Some(reason) = blocker { p class="mt-1 text-xs font-normal text-muted" { (reason) } }
            },
        ),
        (
            "Topic",
            html! {
                div x-data="{ editing: false }" {
                    div x-show="!editing" class="flex items-center gap-1" {
                        span class={"min-w-0 break-words" @if task.title_source == TitleSource::Placeholder { " italic text-muted" }} { (task.title) }
                        button type="button" aria-label="Rename chat" "x-on:click"="editing = true; $nextTick(() => $refs.title.focus())" class=(ICON_BUTTON_CLASS) { (icon(Icon::Pencil)) }
                    }
                    form x-show="editing" x-cloak hx-post=(rename) hx-target="#chat-overview" hx-swap="outerMorph"
                        "x-on:keydown.escape"="editing = false; $refs.title.value = $refs.title.defaultValue"
                        class="flex flex-wrap items-center gap-2" {
                        label class="sr-only" for="title" { "Topic" }
                        input id="title" name="title" type="text" value=(task.title) maxlength=(TITLE_MAX_CHARS) required x-ref="title"
                            class={(INPUT_CLASS) " min-w-0 flex-1"};
                        button type="submit" class=(action_class(ButtonKind::Primary)) { "Save" }
                    }
                }
            },
        ),
        (
            "Project",
            html! { a href={"/projects/" (task.project_key)} class="text-accent-text underline" { (task.project_name) } },
        ),
        (
            "Persona",
            html! { (task.persona_display_name) " " span class="font-normal text-muted" { "· " (harness_label(&task.persona_harness)) @if let Some(model) = &task.persona_model { " · " (model) } } },
        ),
        (
            "Key",
            html! { span class="font-mono text-xs" { (task.key) } },
        ),
        (
            "Checkout",
            html! { span class="font-mono text-xs" { (base_branch) } " " span class="font-normal text-muted" { "read-only" } },
        ),
    ];
    html! {
        div id="chat-overview" class="md:shrink-0" {
            div class="ws-strip flex-col items-center gap-3 p-2" {
                button type="button" class=(ICON_BUTTON_CLASS) aria-label="Show details" title="Show details" "x-on:click"="setCollapsed(false)" { (icon(Icon::PanelRight)) }
                span class={"size-3 rounded-full " (Tone::of_status(task.status).dot_class())}
                    role="img" aria-label=(format!("Status: {status_name}")) title=(status_name) {}
            }
            div class="ws-details-full" {
                (section("Agent", html! {
                    div class="flex items-start gap-3" {
                        (avatar(&initials(&task.persona_display_name)))
                        div class="min-w-0" {
                            p class="font-medium" { (task.persona_display_name) }
                            @if !task.persona_description.is_empty() {
                                p class="text-sm text-muted" { (task.persona_description) }
                            }
                        }
                    }
                }))
                (section("Now", html! {
                    p class="flex items-center gap-2 font-medium" {
                        span aria-hidden="true" class={"size-2 shrink-0 rounded-full " (Tone::of_status(task.status).dot_class())} {}
                        span class="break-words" { @if task.status_line.is_empty() { (status_name) } @else { (task.status_line) } }
                    }
                }))
                div class="border-b border-line px-5 py-4" {
                    (definition_list(&properties))
                    @if error.is_some() { div class="mt-3" { (field_error(error)) } }
                }
            }
        }
    }
}

/// The details panel: bottom sheet, slide-over or column depending on the width, with the tabs
/// Overview, Subtasks and Artifacts.
pub(super) fn details_column(
    task: &TaskDetail,
    base_branch: &str,
    runs: &[RunView],
    subtasks: &[SubtaskItem],
    artifacts: &[ArtifactCard],
) -> Markup {
    details_panel_with_heading(
        "Details",
        chat_details_heading(subtasks.len(), artifacts.len(), false),
        html! {
            div id="panel-overview" role="tabpanel" aria-labelledby="tab-overview" class="ws-overview" {
                (overview_region(task, base_branch, None))
                (runs_region(&task.key, runs, task.persona_model.as_deref()))
            }
            (crate::task_pages::subtasks_panel(subtasks))
            (crate::artifacts::tab_panel(&task.key, artifacts))
        },
    )
}

/// The values of the new chat form.
#[derive(Debug, Default, Clone, serde::Deserialize)]
#[serde(default)]
pub(crate) struct ChatForm {
    pub project: String,
    pub persona: String,
    pub message: String,
}

/// The new chat dialog: the project is fixed, the persona is picked from a list with harness,
/// model and description.
pub(super) fn compose_dialog(
    project: &ChatProject,
    personas: &[PersonaRecord],
    values: &ChatForm,
    errors: &FormErrors,
) -> Markup {
    dialog_with(
        "composer",
        "New chat",
        DialogStyle::Bare,
        compose_form(project, personas, values, errors),
    )
}

/// The form of [`compose_dialog`], also the answer to an invalid submission.
pub(super) fn compose_form(
    project: &ChatProject,
    personas: &[PersonaRecord],
    values: &ChatForm,
    errors: &FormErrors,
) -> Markup {
    let chosen = personas
        .iter()
        .find(|p| p.id.to_string() == values.persona)
        .or(personas.first());
    html! {
        form id="composer-form" class="space-y-2" hx-post="/chats" hx-target="this" hx-swap="outerHTML"
            "x-on:keydown.ctrl.enter.prevent"="$el.requestSubmit()" "x-on:keydown.meta.enter.prevent"="$el.requestSubmit()" {
            input type="hidden" name="project" value=(project.key);
            (form_error(errors))
            div class="flex items-center gap-2 px-1" {
                span class="inline-flex min-h-11 items-center rounded-[10px] border border-line bg-panel px-3 text-sm text-muted lg:min-h-8" title="The project of the chat" { (project.name) }
                div class="ml-auto" { (dialog_close()) }
            }
            div class="relative rounded-2xl border border-line bg-panel p-3 shadow-lg focus-within:border-line-strong" {
                label for="message" class="sr-only" { "Message" }
                textarea id="message" name="message" rows="5" placeholder="What do you want to talk about?"
                    class="w-full resize-none bg-transparent text-base text-ink placeholder:text-faint focus-visible:outline-none" autofocus { (values.message) }
                (field_error(errors.get("message")))
                div class="mt-2 flex items-center gap-2" {
                    (persona_picker(personas, chosen))
                    button type="submit" aria-label="Start chat" title="Start chat" class={(ROUND_ACTION_CLASS) " ml-auto"} { (icon(Icon::ArrowUp)) }
                }
                (field_error(errors.get("persona")))
            }
        }
    }
}

/// The persona picker: a button with the chosen persona that opens the list of all personas,
/// each with harness, model and description.
fn persona_picker(personas: &[PersonaRecord], chosen: Option<&PersonaRecord>) -> Markup {
    let name = chosen.map(|p| p.display_name.as_str()).unwrap_or_default();
    html! {
        div class="relative" x-data={"{ open: false, name: " (serde_json::json!(name)) " }"} "x-on:keydown.escape.stop"="open = false" "x-on:click.outside"="open = false" {
            button type="button" aria-haspopup="listbox" ":aria-expanded"="open" "x-on:click"="open = !open"
                class="inline-flex min-h-11 items-center gap-1 rounded-full bg-transparent px-3 text-sm text-ink hover:bg-sunk lg:min-h-8" {
                span x-text="name" { (name) } (icon(Icon::ChevronDown))
            }
            fieldset x-show="open" x-cloak
                class="absolute bottom-full left-0 z-20 mb-2 max-h-80 w-[min(22rem,80vw)] overflow-y-auto rounded-xl border border-line bg-panel p-1 shadow-lg" {
                legend class="sr-only" { "Persona" }
                @for persona in personas {
                    label class="flex cursor-pointer items-start gap-2 rounded-lg p-2 text-sm hover:bg-sunk has-[:checked]:bg-accent-soft" {
                        input type="radio" name="persona" value=(persona.id) checked[chosen.is_some_and(|c| c.id == persona.id)]
                            "x-on:change"={"name = " (serde_json::json!(persona.display_name)) "; open = false"} class="mt-1";
                        span class="min-w-0" {
                            span class="block font-medium" { (persona.display_name) }
                            span class="block text-xs text-muted" { (harness_label(&persona.harness)) " · " (persona.model) }
                            @if !persona.description.is_empty() { span class="block text-xs text-muted" { (persona.description) } }
                        }
                    }
                }
            }
        }
    }
}

/// The hint of the composer when there is nothing to choose from.
pub(super) fn nothing_to_choose() -> Markup {
    dialog_with(
        "composer",
        "New chat",
        DialogStyle::Bare,
        html! {
            div class="rounded-2xl border border-line bg-panel p-4 shadow-lg" {
                div class="flex justify-end" { (dialog_close()) }
                p class="py-2 text-sm text-muted" { "Create a persona first." }
            }
        },
    )
}
