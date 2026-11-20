//! Cluster services are highlighted and linked like host rules.

use df_db::EntryKind;

use super::app_with_egress;
use super::rules::row_of;
use crate::testkit::{TestApp, seed, text};

async fn add_service(app: &TestApp, namespace: &str, service: &str) {
    let (principal, roles) = crate::testkit::actor();
    let actor = df_db::catalog::Actor {
        principal: &principal,
        roles: &roles,
    };
    let kind = EntryKind::ClusterService {
        namespace: namespace.into(),
        service: service.into(),
        ports: vec![80],
        allow_dns: false,
    };
    df_db::add_entry(&app.pool, actor, None, &kind)
        .await
        .unwrap();
}

#[tokio::test]
async fn highlighting_a_cluster_service_marks_its_row_and_opens_its_page() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    for n in 1..=26 {
        add_service(&app, "ns", &format!("s{n:02}")).await;
    }

    let page = text(app.get("/egress/allowlist?highlight=ns/s26", &cookie).await).await;

    assert_eq!(page.matches("aria-current=\"true\"").count(), 1, "{page}");
    assert!(page.contains("Page 2 of 2"), "{page}");
    assert!(
        row_of(&page, ">ns/s26<").contains("bg-accent-soft"),
        "{page}"
    );
}

#[tokio::test]
async fn an_instance_cluster_service_links_to_its_highlighted_row_on_the_rules_page() {
    let app = app_with_egress(vec![]).await;
    seed(&app.pool).await;
    let cookie = app.login().await;
    add_service(&app, "nativelink", "scheduler").await;

    let page = text(app.get_htmx("/projects/DF/egress", &cookie).await).await;

    assert!(
        page.contains("<a href=\"/egress/allowlist?highlight=nativelink/scheduler\""),
        "{page}"
    );
}
