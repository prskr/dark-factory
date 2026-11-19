use std::cell::RefCell;
use std::collections::HashMap;

use serde_json::{Value, json};

use super::*;
use forge_common::log::StderrLog;

const STATUS: &str = "/api/v1/repos/o/r/commits/feature%2Fx/status";

/// Answers by path without query (404 for anything unknown) and records the requests.
#[derive(Default)]
struct Routes {
    responses: RefCell<HashMap<String, Response>>,
    requests: RefCell<Vec<Request>>,
}

use super::super::Response;

impl Transport for &Routes {
    fn send(&self, request: Request) -> Result<Response, Error> {
        let path = request
            .path
            .split('?')
            .next()
            .unwrap_or_default()
            .to_string();
        let page_two = request.path.contains("page=2");
        self.requests.borrow_mut().push(request);
        let found = self.responses.borrow().get(&path).cloned();
        // Listings of statuses have a second, empty page.
        Ok(match found {
            // As Forgejo does: `null`, not `[]`.
            Some(_) if page_two => json_response(200, &json!({"statuses": null})),
            Some(r) => r,
            None => json_response(404, &json!({"message": format!("no route {path}")})),
        })
    }
}

impl Routes {
    fn get(&self, path: &str, body: &Value) {
        self.responses
            .borrow_mut()
            .insert(path.to_string(), json_response(200, body));
    }

    fn version(&self, version: &str) {
        self.get("/api/v1/version", &json!({ "version": version }));
    }

    fn paths(&self) -> Vec<String> {
        self.requests
            .borrow()
            .iter()
            .map(|r| r.path.clone())
            .collect()
    }
}

fn json_response(status: u16, body: &Value) -> Response {
    Response {
        status,
        retry_after: None,
        body: body.to_string().into_bytes(),
    }
}

fn repo() -> RepoRef {
    RepoRef {
        owner: "o".into(),
        name: "r".into(),
    }
}

fn forgejo(routes: &Routes) -> Forgejo<&Routes, StderrLog> {
    Forgejo::new(routes, StderrLog, Vec::new())
}

fn statuses() -> Value {
    json!({
        "state": "failure", "sha": "abc123",
        "statuses": [
            {"context": "ci / test (pull_request)", "status": "failure", "target_url": "https://git/o/r/actions/runs/5/jobs/1"},
            {"context": "ci / lint (pull_request)", "status": "success", "target_url": ""},
            {"context": "deploy-preview", "status": "pending", "target_url": "https://preview"},
        ]
    })
}

fn actions(routes: &Routes) {
    routes.get(
        "/api/v1/repos/o/r/actions/runs",
        &json!({"workflow_runs": [
            {"id": 5, "name": "ci", "path": ".forgejo/workflows/ci.yml", "event": "pull_request"}
        ]}),
    );
    routes.get(
        "/api/v1/repos/o/r/actions/runs/5/jobs",
        &json!({"jobs": [{"id": 812, "name": "test"}, {"id": 813, "name": "lint"}]}),
    );
}

#[test]
fn v16_checks_carry_the_job_id_of_forgejo_actions_jobs() -> anyhow::Result<()> {
    let routes = Routes::default();
    routes.version("16.0.0+gitea-1.22.0");
    routes.get(STATUS, &statuses());
    actions(&routes);

    let status = forgejo(&routes).ci_status(&repo(), "feature/x")?;

    assert_eq!(status.state, CiState::Failure);
    let checks: Vec<_> = status
        .checks
        .iter()
        .map(|c| (c.name.as_str(), c.state, c.job_id, c.url.as_deref()))
        .collect();
    assert_eq!(
        checks,
        [
            (
                "ci / test (pull_request)",
                CiState::Failure,
                Some(812),
                Some("https://git/o/r/actions/runs/5/jobs/1")
            ),
            (
                "ci / lint (pull_request)",
                CiState::Success,
                Some(813),
                None
            ),
            (
                "deploy-preview",
                CiState::Pending,
                None,
                Some("https://preview")
            ),
        ]
    );
    assert!(
        routes
            .paths()
            .contains(&"/api/v1/repos/o/r/actions/runs?head_sha=abc123&limit=50".to_string())
    );
    Ok(())
}

#[test]
fn v15_checks_have_state_and_url_without_job_id() -> anyhow::Result<()> {
    let routes = Routes::default();
    routes.version("15.0.1+gitea-1.22.0");
    routes.get(STATUS, &statuses());

    let status = forgejo(&routes).ci_status(&repo(), "feature/x")?;

    assert_eq!(status.state, CiState::Failure);
    assert!(status.checks.iter().all(|c| c.job_id.is_none()));
    assert_eq!(status.checks.len(), 3);
    assert!(!routes.paths().iter().any(|p| p.contains("/actions/")));
    Ok(())
}

#[test]
fn a_repository_without_actions_still_has_its_statuses() -> anyhow::Result<()> {
    let routes = Routes::default();
    routes.version("16.0.0");
    routes.get(STATUS, &statuses());
    // `/actions/runs` is not routed: Forgejo answers 404.

    let status = forgejo(&routes).ci_status(&repo(), "feature/x")?;

    assert_eq!(status.checks.len(), 3);
    assert!(status.checks.iter().all(|c| c.job_id.is_none()));
    Ok(())
}

#[test]
fn a_ref_without_statuses_is_none_and_asks_nothing_else() -> anyhow::Result<()> {
    let routes = Routes::default();
    routes.get(STATUS, &json!({"state": "", "sha": "abc", "statuses": []}));

    let status = forgejo(&routes).ci_status(&repo(), "feature/x")?;

    assert_eq!((status.state, status.checks.len()), (CiState::None, 0));
    assert_eq!(routes.paths().len(), 1);
    Ok(())
}

#[test]
fn pending_without_failures_is_pending_and_all_success_is_success() {
    let check = |state| Check {
        name: String::new(),
        state,
        url: None,
        job_id: None,
    };
    assert_eq!(
        combined_state(&[check(CiState::Success), check(CiState::Pending)]),
        CiState::Pending
    );
    assert_eq!(combined_state(&[check(CiState::Success)]), CiState::Success);
}

#[test]
fn an_unknown_ref_is_not_found() {
    let routes = Routes::default();
    let err = forgejo(&routes).ci_status(&repo(), "nope").unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err:?}");
}

/// Serves `body` for the job log and records the request.
struct LogServer {
    body: Vec<u8>,
    requests: RefCell<Vec<Request>>,
}

impl Transport for &LogServer {
    fn send(&self, request: Request) -> Result<Response, Error> {
        let version = request.path == "/api/v1/version";
        self.requests.borrow_mut().push(request);
        let body = if version {
            br#"{"version": "16.1.0"}"#.to_vec()
        } else {
            self.body.clone()
        };
        Ok(Response {
            status: 200,
            retry_after: None,
            body,
        })
    }
}

#[test]
fn a_log_is_read_with_a_range_for_its_tail() -> anyhow::Result<()> {
    let server = LogServer {
        body: b"line 1\nerror: boom\n".to_vec(),
        requests: RefCell::default(),
    };
    let log = Forgejo::new(&server, StderrLog, Vec::new()).job_log(&repo(), 812)?;

    assert_eq!(log.text, "line 1\nerror: boom\n");
    assert!(!log.head_dropped);
    let requests = server.requests.borrow();
    let request = requests.last().expect("a request");
    assert_eq!(request.path, "/api/v1/repos/o/r/actions/jobs/812/logs");
    assert_eq!(request.headers, [("range", "bytes=-4194304".to_string())]);
    Ok(())
}

#[test]
fn a_log_over_4_mib_keeps_its_end_and_says_so() -> anyhow::Result<()> {
    let mut body = vec![b'a'; 1024];
    body.extend(vec![b'b'; log_tail::MAX_BYTES]);
    let server = LogServer {
        body,
        requests: RefCell::default(),
    };

    let log = Forgejo::new(&server, StderrLog, Vec::new()).job_log(&repo(), 813)?;

    assert!(log.head_dropped);
    assert_eq!(log.text.len(), log_tail::MAX_BYTES);
    assert!(log.text.bytes().all(|b| b == b'b'));
    Ok(())
}

#[test]
fn logs_on_v15_are_unsupported_and_name_v16() {
    let routes = Routes::default();
    routes.version("15.0.0+gitea-1.22.0");

    let err = forgejo(&routes).job_log(&repo(), 812).unwrap_err();

    assert!(
        matches!(&err, Error::Unsupported(m) if m.contains("v16")),
        "{err:?}"
    );
    assert_eq!(routes.paths(), ["/api/v1/version"]);
}

#[test]
fn an_expired_log_is_not_found() {
    let routes = Routes::default();
    routes.version("16.0.0");
    let err = forgejo(&routes).job_log(&repo(), 1).unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "{err:?}");
}
