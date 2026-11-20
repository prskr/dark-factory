//! The header of the rules page keeps its actions on the right, like the other list pages.

use super::app_with_egress;
use crate::testkit::text;

/// The opening tag of the `<header>` that contains `marker`.
fn header_around<'a>(page: &'a str, marker: &str) -> &'a str {
    let at = page.find(marker).expect(marker);
    let start = page[..at]
        .rfind("<header")
        .expect("a header before the marker");
    &page[start..=(start + page[start..].find('>').unwrap())]
}

#[tokio::test]
async fn the_header_actions_stay_in_the_row_at_the_right_instead_of_wrapping_left_aligned() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;

    let page = text(app.get("/egress/allowlist", &cookie).await).await;

    let header = header_around(&page, "External hosts and cluster services");
    assert!(
        header.contains("justify-between") && !header.contains("flex-wrap"),
        "a wrapping header left-aligns the actions on their own line: {header}"
    );
}
