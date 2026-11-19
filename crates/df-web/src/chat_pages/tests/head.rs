//! The header of an open chat, rendered without a database: the order of its controls decides
//! where they sit between 768 and 1279 px, where the persona block is hidden.

use df_core::{Principal, TaskStatus, TitleSource, UserId};
use df_db::task_views::TaskDetail;
use uuid::Uuid;

use crate::chat_pages::head_region;
use crate::testkit::tag_around;

fn chat() -> TaskDetail {
    TaskDetail {
        id: Uuid::nil(),
        key: "DF-1-CHAT".to_owned(),
        title: "Plan the Helm chart release".to_owned(),
        title_source: TitleSource::User,
        description: String::new(),
        project_key: "DF".to_owned(),
        project_name: "Dark Factory".to_owned(),
        repo_url: "https://forge.example/df.git".to_owned(),
        issue_tracker_url: None,
        persona_name: "Linus".to_owned(),
        persona_harness: "claude-code".to_owned(),
        persona_model: None,
        persona_description: String::new(),
        issue_number: None,
        status: TaskStatus::Waiting,
        status_line: String::new(),
        pr_number: None,
        pr_url: None,
        pr_title: None,
        pr_attached: false,
        archived_at: None,
        has_pending_request: false,
        branch: "factory/task/DF-1-CHAT".to_owned(),
        resume_at: None,
        auto_resumes: 0,
        created_by: Principal::human(UserId::new(), "Owner"),
        creator_task_key: None,
        parent_key: None,
        parent_title: None,
        parent_persona_name: None,
        base_task_key: None,
        base_task_title: None,
    }
}

// From 768 px nothing in the row grows, so two `ml-auto` items would split the free space and put
// "Details" mid-header. "New chat" alone pushes both buttons to the right edge; on phones the
// `flex-1` persona block does.
#[test]
fn details_comes_after_new_chat_and_only_new_chat_pushes_the_buttons_right() {
    let html = head_region(&chat()).into_string();

    let new_chat = html.find("New chat").expect("New chat button");
    let details = html.find("aria-label=\"Details\"").expect("Details button");
    assert!(new_chat < details, "{html}");
    assert_eq!(html.matches("ml-auto").count(), 1, "{html}");
    let new_chat_tag = tag_around(&html, "hx-get=\"/chats/compose");
    assert!(new_chat_tag.contains("ml-auto"), "{new_chat_tag}");
}
