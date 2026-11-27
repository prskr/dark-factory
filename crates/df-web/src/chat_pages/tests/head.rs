//! The header of an open chat, rendered without a database: the order of its controls decides
//! where they sit between 768 and 1279 px, where the persona block is hidden.

use crate::chat_pages::head_region;
use crate::testkit::{tag_around, task_detail};

// From 768 px nothing in the row grows, so two `ml-auto` items would split the free space and put
// "Details" mid-header. "New chat" alone pushes both buttons to the right edge; on phones the
// `flex-1` persona block does.
#[test]
fn details_comes_after_new_chat_and_only_new_chat_pushes_the_buttons_right() {
    let html = head_region(&task_detail("DF-1-CHAT")).into_string();

    let new_chat = html.find("New chat").expect("New chat button");
    let details = html.find("aria-label=\"Details\"").expect("Details button");
    assert!(new_chat < details, "{html}");
    assert_eq!(html.matches("ml-auto").count(), 1, "{html}");
    let new_chat_tag = tag_around(&html, "hx-get=\"/chats/compose");
    assert!(new_chat_tag.contains("ml-auto"), "{new_chat_tag}");
}

#[test]
fn the_chat_head_is_a_focus_target_named_by_a_screen_reader_only_heading() {
    let task = task_detail("DF-1-CHAT");

    let html = head_region(&task).into_string();

    // The frame is tested once, in `components::detail_head`.
    assert!(
        html.starts_with("<header id=\"chat-head\" tabindex=\"-1\""),
        "{html}"
    );
    assert!(
        html.contains(&format!(
            "<h2 id=\"detail-title\" class=\"sr-only\">DF-1-CHAT {}</h2>",
            task.title
        )),
        "{html}"
    );
}
