//! Shared fixtures of the tests; the tests themselves live in the topic modules.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::http::StatusCode;
use df_proto::gateway::v1::{FlowEvent, Verdict};
use df_proto::pbjson_types::Timestamp;
use futures::StreamExt;
use futures::stream::{self, BoxStream};

use super::log::{Context, Filter, Viewer};
use super::{FlowSource, LookupError, ServiceLookup};
use crate::testkit::{TestApp, form};

mod allow;
mod deny_log;
mod deny_rules;
mod flows;
mod icons;
mod log;
mod paging;
mod rules;

/// Replays `events` to every viewer and then stays open.
struct FakeFlows(Vec<FlowEvent>);

impl FlowSource for FakeFlows {
    fn subscribe(&self) -> BoxStream<'static, FlowEvent> {
        let backlog_end = FlowEvent {
            backlog_end: true,
            ..Default::default()
        };
        stream::iter(self.0.clone())
            .chain(stream::iter([backlog_end]))
            .chain(stream::pending())
            .boxed()
    }
}

/// Knows exactly one Service: `nativelink/scheduler`.
struct FakeServices;

#[async_trait]
impl ServiceLookup for FakeServices {
    async fn exists(&self, namespace: &str, service: &str) -> Result<bool, LookupError> {
        Ok((namespace, service) == ("nativelink", "scheduler"))
    }
}

fn denied(host: &str, project: &str, second: i64) -> FlowEvent {
    FlowEvent {
        time: Some(Timestamp {
            seconds: second,
            nanos: 0,
        }),
        host: host.into(),
        port: 443,
        method: "CONNECT".into(),
        project_key: project.into(),
        task_key: format!("{project}-1"),
        verdict: Verdict::Denied.into(),
        reason: "not-allowlisted".into(),
        ..Default::default()
    }
}

fn allowed(host: &str, project: &str, second: i64) -> FlowEvent {
    FlowEvent {
        verdict: Verdict::Allowed.into(),
        reason: String::new(),
        ..denied(host, project, second)
    }
}

async fn app_with_egress(events: Vec<FlowEvent>) -> TestApp {
    TestApp::with_egress(
        df_core::routing::ForgeRoutes::default(),
        Some(Arc::new(FakeFlows(events))),
        Some(Arc::new(FakeServices)),
    )
    .await
}

/// A cluster that cannot be asked.
struct BrokenServices;

#[async_trait]
impl ServiceLookup for BrokenServices {
    async fn exists(&self, _: &str, _: &str) -> Result<bool, LookupError> {
        Err(LookupError::NamespaceNotAllowed("other".into()))
    }
}

/// Adds an `http-host` entry through the form.
async fn add_host(app: &TestApp, cookie: &str, project: &str, host: &str, port: &str) {
    let response = app
        .post(
            "/egress/allowlist/host",
            &form(&[("project", project), ("host", host), ("port", port)]),
            cookie,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK, "{host}");
}

/// Adds a task-scoped `http-host` entry on port 443 directly in the database.
async fn add_task_host(app: &TestApp, task: uuid::Uuid, host: &str) -> df_db::EntryRecord {
    let (principal, roles) = crate::testkit::actor();
    let actor = df_db::catalog::Actor {
        principal: &principal,
        roles: &roles,
    };
    let mut tx = app.pool.begin().await.unwrap();
    let entry = df_db::create_http_host_entry(
        &mut tx,
        df_db::HostScope::Task(task),
        host,
        443,
        actor,
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    entry
}

/// Adds a deny rule directly in the database.
async fn add_deny_rule(app: &TestApp, host: &str) {
    let (principal, roles) = crate::testkit::actor();
    let actor = df_db::catalog::Actor {
        principal: &principal,
        roles: &roles,
    };
    df_db::egress::add_deny_rule(&app.pool, host, actor)
        .await
        .unwrap();
}

/// The content of the `<hx-partial>` for `target` in a message; `None` if the message has none.
fn part<'a>(body: &'a str, target: &str) -> Option<&'a str> {
    inner(
        body,
        &format!("<hx-partial hx-target=\"{target}\""),
        "</hx-partial>",
    )
}

/// The text between the end of the first tag that starts with `open` and the next `end`.
fn inner<'a>(html: &'a str, open: &str, end: &str) -> Option<&'a str> {
    let at = html.find(open)?;
    let content = &html[at + html[at..].find('>')? + 1..];
    Some(&content[..content.find(end)?])
}

/// A viewer without project hosts and deny rules.
fn new_viewer(filter: Filter) -> Viewer {
    Viewer::new(filter, Context::default())
}

/// What the listener delivered before a marker that is published after the action under test:
/// notifications arrive in commit order, so a wrongly published `egress_policy` would show up
/// before the marker.
async fn notifications_before_marker(
    app: &TestApp,
    notifications: &mut tokio::sync::broadcast::Receiver<df_db::Notification>,
) -> Vec<df_db::Notification> {
    let marker = df_db::Notification::TaskChanged {
        task_id: df_core::TaskId::new(),
    };
    df_db::publish(&app.pool, &marker).await.unwrap();
    let mut before = Vec::new();
    loop {
        let received = tokio::time::timeout(Duration::from_secs(5), notifications.recv())
            .await
            .expect("the marker never arrived")
            .unwrap();
        if received == marker {
            return before;
        }
        before.push(received);
    }
}

/// A viewer that knows the deny rules `patterns`.
fn viewer_with_deny(patterns: &[&str]) -> Viewer {
    let deny = patterns.iter().map(ToString::to_string).collect();
    Viewer::new(
        Filter::default(),
        Context::new(crate::components::ProjectHosts::default(), deny),
    )
}

fn live(filter: Filter, batch: &[FlowEvent]) -> Option<String> {
    new_viewer(filter).live_message(batch)
}

/// Parses a query string like the `Query` extractor does.
fn filter_from_query(query: &str) -> Filter {
    let uri: axum::http::Uri = format!("/x?{query}").parse().unwrap();
    axum::extract::Query::<Filter>::try_from_uri(&uri)
        .unwrap()
        .0
}

/// An event of flow `id` of pod `gw-0`.
fn flow(mut event: FlowEvent, id: u64) -> FlowEvent {
    event.gateway_pod = "gw-0".into();
    event.flow_id = id;
    event
}

/// The source of the embedded Alpine component of the log page.
pub(in crate::egress) fn component_script() -> String {
    crate::assets::script_source("egress-log.js")
}
