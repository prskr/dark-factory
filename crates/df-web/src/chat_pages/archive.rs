//! The archive modal of a project (`specs/chat-ui/spec.md`, "Archive modal"): a searchable list of
//! the archived chats; a selected chat shows its read-only transcript and its artifacts, an
//! opened artifact replaces the cards. Everything the modal does is a request for its body.

use axum::Extension;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use df_core::TaskId;
use df_db::chat_views::ChatListItem;
use df_db::task_views::TaskDetail;
use maud::{Markup, html};
use serde::Deserialize;

use crate::WebState;
use crate::auth::CurrentUser;
use crate::components::{
    ButtonKind, DialogStyle, ICON_BUTTON_CLASS, Icon, action_class, avatar, dialog_with, icon,
    initials,
};
use crate::responses::{internal_error, not_found};
use crate::transcript::render_transcript;

/// The query of the modal: what it shows. `part=body` asks for the body alone (a request from
/// inside the modal); without it the answer is the dialog.
#[derive(Debug, Default, Deserialize)]
pub(super) struct ArchiveQuery {
    project: Option<String>,
    q: Option<String>,
    chat: Option<String>,
    tab: Option<String>,
    artifact: Option<String>,
    part: Option<String>,
}

/// Which tab of a selected chat is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Transcript,
    Artifacts,
}

impl Tab {
    fn parse(tab: Option<&str>) -> Self {
        if tab == Some("artifacts") {
            Self::Artifacts
        } else {
            Self::Transcript
        }
    }
}

/// Where the modal stands, as the links inside it keep it.
struct Stand<'a> {
    project: &'a str,
    q: &'a str,
}

impl Stand<'_> {
    fn url(&self, chat: Option<&str>, tab: Option<&str>, artifact: Option<&str>) -> String {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query
            .append_pair("part", "body")
            .append_pair("project", self.project);
        if !self.q.is_empty() {
            query.append_pair("q", self.q);
        }
        for (name, value) in [("chat", chat), ("tab", tab), ("artifact", artifact)] {
            if let Some(value) = value {
                query.append_pair(name, value);
            }
        }
        format!("/chats/archive?{}", query.finish())
    }
}

/// `GET /chats/archive?project=KEY[&q=..][&chat=KEY[&tab=artifacts[&artifact=NAME]]]`.
pub(super) async fn archive_modal(
    State(state): State<WebState>,
    Extension(_user): Extension<CurrentUser>,
    Query(query): Query<ArchiveQuery>,
) -> Response {
    let project_key = query.project.clone().unwrap_or_default();
    let projects = match df_db::chat_views::list_chat_projects(&state.pool).await {
        Ok(projects) => projects,
        Err(e) => return internal_error(&e),
    };
    let Some(project) = projects.into_iter().find(|p| p.key == project_key) else {
        return not_found("The project");
    };
    let q = query.q.clone().unwrap_or_default();
    let chats = match df_db::chat_views::list_chats(&state.pool, &project.key, true, Some(&q)).await
    {
        Ok(chats) => chats,
        Err(e) => return internal_error(&e),
    };
    let stand = Stand {
        project: &project.key,
        q: &q,
    };
    let selected = match selected_chat(&state, &query, &project.key).await {
        Ok(selected) => selected,
        Err(response) => return *response,
    };
    let body = body(&state, &stand, &chats, project.archived, selected, &query).await;
    let body = match body {
        Ok(body) => body,
        Err(response) => return *response,
    };
    if query.part.as_deref() == Some("body") {
        return body.into_response();
    }
    dialog_with("archive", "Archived chats", DialogStyle::Wider, body).into_response()
}

/// The chat the modal shows, if one is selected: an archived chat of the project.
async fn selected_chat(
    state: &WebState,
    query: &ArchiveQuery,
    project: &str,
) -> Result<Option<TaskDetail>, Box<Response>> {
    let Some(key) = query
        .chat
        .as_deref()
        .filter(|key| df_core::is_chat_key(key))
    else {
        return Ok(None);
    };
    match df_db::task_views::get_task_detail_by_key(&state.pool, key).await {
        Ok(Some(chat)) if chat.archived_at.is_some() && chat.project_key == project => {
            Ok(Some(chat))
        }
        Ok(_) => Err(Box::new(not_found("The archived chat"))),
        Err(e) => Err(Box::new(internal_error(&e))),
    }
}

async fn body(
    state: &WebState,
    stand: &Stand<'_>,
    chats: &[ChatListItem],
    total: i64,
    selected: Option<TaskDetail>,
    query: &ArchiveQuery,
) -> Result<Markup, Box<Response>> {
    let current = selected.as_ref().map(|chat| chat.key.as_str());
    let pane = match &selected {
        Some(chat) => Some(chat_pane(state, stand, chat, query).await?),
        None => None,
    };
    Ok(html! {
        div id="archive-body" x-data=(format!("archiveModal({})", serde_json::json!({ "selected": selected.is_some() })))
            class="flex h-[calc(100dvh-4.625rem)] min-h-0 md:h-[75dvh]" {
            aside ":class"="chat ? 'max-md:hidden' : ''"
                class={"flex min-h-0 flex-col border-r border-line " (if selected.is_some() { "w-full md:w-80 md:shrink-0" } else { "w-full" })} {
                div class="border-b border-line p-3" {
                    label class="sr-only" for="archive-search" { "Search archived chats" }
                    input id="archive-search" name="q" type="search" value=(stand.q) placeholder="Search topic, persona and messages"
                        hx-get=(Stand { project: stand.project, q: "" }.url(current, None, None))
                        hx-trigger="input changed delay:250ms, search" hx-target="#archive-body" hx-swap="outerHTML"
                        class="min-h-11 w-full rounded-[10px] border border-line-strong bg-transparent px-3 text-sm lg:min-h-9";
                    p class="mt-1 text-xs text-faint" { (chats.len()) " of " (total) " archived" }
                }
                ul class="min-h-0 flex-1 overflow-y-auto" {
                    @for chat in chats {
                        @let url = stand.url(Some(&chat.key), None, None);
                        li {
                            a href=(url) hx-get=(url) hx-target="#archive-body" hx-swap="outerHTML"
                                "x-on:click"="chat = true"
                                aria-current=[(current == Some(chat.key.as_str())).then_some("true")]
                                class="block border-b border-line px-4 py-3 hover:bg-sunk aria-[current=true]:bg-accent-soft" {
                                div class="flex items-center gap-2" {
                                    (avatar(&initials(&chat.persona_name)))
                                    span class="min-w-0 truncate text-sm font-medium" { (chat.persona_name) }
                                    span class="ml-auto shrink-0 font-mono text-xs text-faint" { (chat.key) }
                                }
                                div class="mt-1 truncate text-[15px] font-medium" { (chat.title) }
                                @if !chat.last_message.is_empty() { div class="truncate text-xs text-muted" { (chat.last_message) } }
                                div class="mt-1 flex items-center gap-2 text-xs text-faint" {
                                    span { (chat.artifacts) " artifacts" }
                                    @if let Some(at) = chat.archived_at { span class="ml-auto" { "archived " time datetime=(at.to_rfc3339()) { (at.format("%b %d")) } } }
                                }
                            }
                        }
                    }
                    @if chats.is_empty() {
                        li class="px-4 py-8 text-center text-sm text-faint" {
                            @if stand.q.is_empty() { "No archived chats." } @else { "No archived chat matches." }
                        }
                    }
                }
            }
            @if let Some(pane) = pane { (pane) }
        }
    })
}

/// The right side: header with Unarchive and the tabs, then the transcript or the artifacts.
async fn chat_pane(
    state: &WebState,
    stand: &Stand<'_>,
    chat: &TaskDetail,
    query: &ArchiveQuery,
) -> Result<Markup, Box<Response>> {
    let id = TaskId::from_uuid(chat.id);
    let tab = Tab::parse(query.tab.as_deref());
    let content = match tab {
        Tab::Transcript => {
            let folder = crate::task_pages::load_folder(state, id, &chat.persona_name)
                .await
                .map_err(|e| Box::new(internal_error(&e)))?;
            // Read-only: the transcript without its message box.
            html! { div class="min-h-0 flex-1 overflow-y-auto p-4" { (render_transcript(folder.items(), &[])) } }
        }
        Tab::Artifacts => {
            artifacts_content(state, stand, chat, id, query.artifact.as_deref()).await?
        }
    };
    let tab_link = |label: &str, target: Tab, name: &str| {
        let url = stand.url(Some(&chat.key), Some(name), None);
        html! {
            a href=(url) hx-get=(url) hx-target="#archive-body" hx-swap="outerHTML" role="tab" aria-selected=(tab == target)
                class={"min-h-11 content-center border-b-2 px-1 text-sm font-medium lg:min-h-9 " (if tab == target { "border-accent text-ink" } else { "border-transparent text-muted" })} { (label) }
        }
    };
    Ok(html! {
        section class="flex min-h-0 min-w-0 flex-1 flex-col" aria-label="Archived chat" {
            header class="flex items-center gap-2 border-b border-line px-3 py-2" {
                button type="button" aria-label="Back to the archived chats" "x-on:click"="chat = false" class={(ICON_BUTTON_CLASS) " md:hidden"} { (icon(Icon::ChevronLeft)) }
                div class="min-w-0 flex-1" {
                    div class="truncate font-semibold" { (chat.title) }
                    div class="truncate text-xs text-muted" { (chat.persona_name) " · " span class="font-mono" { (chat.key) } }
                }
                button type="button" hx-post={"/chats/" (chat.key) "/unarchive"} hx-swap="none"
                    class=(action_class(ButtonKind::Secondary)) { (icon(Icon::Archive)) "Unarchive" }
            }
            div role="tablist" aria-label="Archived chat" class="flex gap-4 border-b border-line px-4" {
                (tab_link("Transcript", Tab::Transcript, "transcript"))
                (tab_link("Artifacts", Tab::Artifacts, "artifacts"))
            }
            (content)
        }
    })
}

/// The Artifacts tab: the cards, or the opened artifact with the way back to them.
async fn artifacts_content(
    state: &WebState,
    stand: &Stand<'_>,
    chat: &TaskDetail,
    id: TaskId,
    artifact: Option<&str>,
) -> Result<Markup, Box<Response>> {
    if let Some(name) = artifact {
        let (_meta, rendered) = crate::artifacts::rendered_artifact(
            state,
            id,
            &chat.key,
            name,
            crate::artifacts::Place::Page,
        )
        .await
        .map_err(|e| Box::new(internal_error(&e)))?
        .ok_or_else(|| Box::new(not_found("The artifact")))?;
        let back = stand.url(Some(&chat.key), Some("artifacts"), None);
        return Ok(html! {
            div class="flex min-h-0 flex-1 flex-col" {
                a href=(back) hx-get=(back) hx-target="#archive-body" hx-swap="outerHTML"
                    class="flex min-h-11 items-center gap-1 border-b border-line px-4 text-sm font-medium text-accent-text" {
                    (icon(Icon::ArrowLeft)) "Artifacts"
                }
                (rendered)
            }
        });
    }
    let cards = crate::artifacts::load_cards(state, id)
        .await
        .map_err(|e| Box::new(internal_error(&e)))?;
    Ok(html! {
        div class="min-h-0 flex-1 overflow-y-auto" {
            @if cards.is_empty() {
                p class="px-4 py-8 text-center text-sm text-faint" { "This chat has no artifacts." }
            } @else {
                (crate::artifacts::card_grid(&chat.key, &cards, |name| stand.url(Some(&chat.key), Some("artifacts"), Some(name))))
            }
        }
    })
}
