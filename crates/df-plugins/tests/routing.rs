//! Forge routing over several Forgejo plugin instances, each behind its own mock forge (specs
//! `forge-routing` and the base URL rules of `plugin-host`).

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use df_core::config::{HostRoute, HostWithApiUrl};
use df_core::routing::RoutingTable;
use df_core::{ForgeClient, IntegrationError, PluginConfig, ProjectId};
use df_plugins::{ForgeRouter, PluginEngine};
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MIB: usize = 1024 * 1024;
const PULLS: &str = "/api/v1/repos/o/r/pulls";
/// Issue number of the test-probe operation reporting its `base-url`.
const PROBE_BASE_URL: u64 = 7;
/// Issue number of the test-probe operation requesting `http://<repo owner>/`.
const PROBE_OTHER_HOST: u64 = 6;

/// A mock forge answering the open pull request lookup of `o/r`, only with the credential `token`
/// of one plugin.
async fn forge(token: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(PULLS))
        .and(header("authorization", format!("token {token}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    server
}

/// `name:port` of the mock forge, the host of its repository URLs.
fn authority(server: &MockServer) -> String {
    server.address().to_string()
}

fn repo_url(server: &MockServer) -> String {
    format!("{}/o/r.git", server.uri())
}

fn claim(host: &str, api_url: &str) -> HostRoute {
    HostRoute::WithApiUrl(HostWithApiUrl {
        host: host.into(),
        api_url: api_url.into(),
    })
}

/// A router over one Forgejo plugin per entry; each plugin's credential is `token-<name>`.
fn router(
    plugins: Vec<(&str, Vec<HostRoute>)>,
    projects: Vec<(ProjectId, String)>,
) -> anyhow::Result<ForgeRouter> {
    router_of(&common::forgejo(), plugins, projects)
}

/// Like [`router`], with every plugin running `component`.
fn router_of(
    component: &[u8],
    plugins: Vec<(&str, Vec<HostRoute>)>,
    projects: Vec<(ProjectId, String)>,
) -> anyhow::Result<ForgeRouter> {
    router_with_unloaded(component, plugins, &[], projects)
}

/// Like [`router_of`]; the plugins in `unloaded` stay in the routing table without a client.
fn router_with_unloaded(
    component: &[u8],
    plugins: Vec<(&str, Vec<HostRoute>)>,
    unloaded: &[&str],
    projects: Vec<(ProjectId, String)>,
) -> anyhow::Result<ForgeRouter> {
    let engine = PluginEngine::new()?;
    let mut configs = BTreeMap::new();
    let mut clients = BTreeMap::new();
    for (name, hosts) in plugins {
        let mut config = PluginConfig::new("/forgejo.wasm");
        config.hosts = hosts;
        configs.insert(name.to_string(), config);
        if unloaded.contains(&name) {
            continue;
        }
        let client = common::plugin_client(
            &engine,
            component,
            &format!("token-{name}"),
            Duration::from_secs(30),
            64 * MIB,
        )?;
        clients.insert(name.to_string(), client);
    }
    Ok(ForgeRouter::new(
        Arc::new(common::ProjectRepos(projects)),
        RoutingTable::from_plugins_allowing_plain_http(&configs)?,
        clients,
    ))
}

async fn find_open(router: &ForgeRouter, project: &ProjectId) -> Result<(), IntegrationError> {
    let found = router
        .find_open_pull_request(project, "factory/task/DF-1", "main")
        .await?;
    assert!(found.is_none());
    Ok(())
}

async fn requests(server: &MockServer) -> usize {
    server.received_requests().await.map_or(0, |r| r.len())
}

#[tokio::test]
async fn two_forges_side_by_side_each_serve_only_their_projects() -> anyhow::Result<()> {
    let (a, b) = (forge("token-a").await, forge("token-b").await);
    let (project_a, project_b) = (ProjectId::new(), ProjectId::new());
    let router = router(
        vec![
            ("a", vec![claim(&authority(&a), &a.uri())]),
            ("b", vec![claim(&authority(&b), &b.uri())]),
        ],
        vec![(project_a, repo_url(&a)), (project_b, repo_url(&b))],
    )?;

    find_open(&router, &project_a).await?;
    assert_eq!((requests(&a).await, requests(&b).await), (1, 0));
    find_open(&router, &project_b).await?;
    assert_eq!((requests(&a).await, requests(&b).await), (1, 1));
    Ok(())
}

#[tokio::test]
async fn claimed_host_goes_to_its_plugin_and_other_hosts_to_the_default() -> anyhow::Result<()> {
    let (claimed, other) = (forge("token-claimer").await, forge("token-fallback").await);
    let (on_claimed, on_other) = (ProjectId::new(), ProjectId::new());
    let router = router(
        vec![
            ("claimer", vec![claim(&authority(&claimed), &claimed.uri())]),
            ("fallback", vec![]),
        ],
        vec![
            (on_claimed, repo_url(&claimed)),
            (on_other, repo_url(&other)),
        ],
    )?;

    find_open(&router, &on_claimed).await?;
    find_open(&router, &on_other).await?;
    assert_eq!((requests(&claimed).await, requests(&other).await), (1, 1));
    Ok(())
}

#[tokio::test]
async fn single_plugin_without_hosts_serves_every_project() -> anyhow::Result<()> {
    let (a, b) = (forge("token-forgejo").await, forge("token-forgejo").await);
    let (project_a, project_b) = (ProjectId::new(), ProjectId::new());
    let router = router(
        vec![("forgejo", vec![])],
        vec![(project_a, repo_url(&a)), (project_b, repo_url(&b))],
    )?;

    find_open(&router, &project_a).await?;
    find_open(&router, &project_b).await?;
    assert_eq!((requests(&a).await, requests(&b).await), (1, 1));
    Ok(())
}

#[tokio::test]
async fn unclaimed_host_without_default_is_a_config_error_naming_the_host() -> anyhow::Result<()> {
    let (claimed, unclaimed) = (forge("token-a").await, forge("token-a").await);
    let project = ProjectId::new();
    let router = router(
        vec![("a", vec![claim(&authority(&claimed), &claimed.uri())])],
        vec![(project, repo_url(&unclaimed))],
    )?;

    let err = find_open(&router, &project).await.unwrap_err();
    assert!(
        matches!(&err, IntegrationError::Config(m) if m.contains(&authority(&unclaimed))),
        "{err}"
    );
    assert_eq!(
        (requests(&claimed).await, requests(&unclaimed).await),
        (0, 0)
    );
    Ok(())
}

#[tokio::test]
async fn host_of_an_unloaded_plugin_is_a_config_error_and_never_reaches_the_default()
-> anyhow::Result<()> {
    // The default would call this server if the claimed host fell through to it.
    let default = forge("token-forgejo").await;
    let project = ProjectId::new();
    let router = router_with_unloaded(
        &common::forgejo(),
        vec![
            ("forgejo", vec![]),
            ("github", vec![HostRoute::Host(authority(&default))]),
        ],
        &["github"],
        vec![(project, repo_url(&default))],
    )?;

    let err = find_open(&router, &project).await.unwrap_err();
    assert!(
        matches!(&err, IntegrationError::Config(m)
            if m.contains("`github`") && m.contains(&authority(&default)) && m.contains("did not load")),
        "{err}"
    );
    assert_eq!(requests(&default).await, 0);
    Ok(())
}

#[tokio::test]
async fn repository_url_with_userinfo_is_a_config_error_without_the_userinfo() -> anyhow::Result<()>
{
    let server = forge("token-forgejo").await;
    let project = ProjectId::new();
    let router = router(
        vec![("forgejo", vec![])],
        vec![(
            project,
            format!("https://user:s3cret@{}/o/r", authority(&server)),
        )],
    )?;

    let err = find_open(&router, &project).await.unwrap_err();
    assert!(
        matches!(&err, IntegrationError::Config(m) if !m.contains("user") && !m.contains("s3cret")),
        "{err}"
    );
    assert_eq!(requests(&server).await, 0);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn plugin_request_to_the_repository_host_instead_of_the_api_url_is_denied()
-> anyhow::Result<()> {
    let (api, repo_host) = (forge("token-a").await, forge("token-a").await);
    let project = ProjectId::new();
    // The probe requests `http://<repo owner>/`: here the repository host itself.
    let router = router_of(
        &common::probe(),
        vec![("a", vec![claim(&authority(&repo_host), &api.uri())])],
        vec![(
            project,
            format!("{}/{}/r.git", repo_host.uri(), authority(&repo_host)),
        )],
    )?;

    let err = router
        .get_issue(&project, PROBE_OTHER_HOST)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, IntegrationError::Transport(m) if m.contains("HttpRequestDenied")),
        "{err}"
    );
    assert_eq!((requests(&api).await, requests(&repo_host).await), (0, 0));
    Ok(())
}

#[tokio::test]
async fn separate_api_host_receives_the_calls_for_the_repository_host() -> anyhow::Result<()> {
    let api = forge("token-a").await;
    let project = ProjectId::new();
    let router = router(
        vec![("a", vec![claim("git.example", &api.uri())])],
        vec![(project, "https://git.example/o/r.git".into())],
    )?;

    find_open(&router, &project).await?;
    assert_eq!(requests(&api).await, 1);
    Ok(())
}

#[tokio::test]
async fn plugin_gets_the_route_api_url_as_base_url() -> anyhow::Result<()> {
    let project = ProjectId::new();
    let router = router_of(
        &common::probe(),
        vec![(
            "a",
            vec![claim("git.example", "https://API.example:443/api/v3/")],
        )],
        vec![(project, "https://git.example/o/r.git".into())],
    )?;

    let issue = router.get_issue(&project, PROBE_BASE_URL).await?;
    assert_eq!(issue.body, "https://api.example/api/v3");
    Ok(())
}

#[tokio::test]
async fn default_plugin_gets_scheme_and_host_of_the_repository_url_as_base_url()
-> anyhow::Result<()> {
    let project = ProjectId::new();
    let router = router_of(
        &common::probe(),
        vec![("a", vec![])],
        vec![(project, "https://git.example:8443/o/r.git".into())],
    )?;

    let issue = router.get_issue(&project, PROBE_BASE_URL).await?;
    assert_eq!(issue.body, "https://git.example:8443");
    Ok(())
}
