//! Test support: an app on a test database, a mock OIDC provider and request helpers.

use std::sync::{Arc, LazyLock, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, Response, header};
use df_core::AuthConfig;
use df_core::PluginConfig;
use df_core::config::HostRoute;
use df_core::routing::{ForgeRoutes, RoutingTable};
use http_body_util::BodyExt;
use openidconnect::core::{
    CoreGenderClaim, CoreJsonWebKeySet, CoreJweContentEncryptionAlgorithm, CoreJwsSigningAlgorithm,
    CoreRsaPrivateSigningKey,
};
use openidconnect::{
    AdditionalClaims, Audience, EndUserName, IdToken, IdTokenClaims, IssuerUrl, JsonWebKeyId,
    Nonce, PrivateSigningKey, StandardClaims, SubjectIdentifier,
};
use rsa::pkcs1::EncodeRsaPrivateKey;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request as MockRequest, Respond, ResponseTemplate};

use crate::auth::OidcClient;
use crate::{WebState, router};

pub(crate) const CLIENT_ID: &str = "dark-factory";

/// Signing key of every mock provider, as PKCS#1 PEM. Generated once per test process: a 2048-bit
/// key takes seconds in an unoptimized build, and most tests start a provider.
static SIGNING_KEY_PEM: LazyLock<String> = LazyLock::new(|| {
    let key = rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).unwrap();
    key.to_pkcs1_pem(rsa::pkcs8::LineEnding::LF)
        .unwrap()
        .to_string()
});

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RolesClaim {
    roles: Vec<String>,
}
impl AdditionalClaims for RolesClaim {}

type TestIdToken = IdToken<
    RolesClaim,
    CoreGenderClaim,
    CoreJweContentEncryptionAlgorithm,
    CoreJwsSigningAlgorithm,
>;
type TestClaims = IdTokenClaims<RolesClaim, CoreGenderClaim>;

/// What the mock provider answers with; tests change it between requests.
#[derive(Debug, Clone)]
pub(crate) struct IdpState {
    pub nonce: Option<String>,
    pub roles: Vec<String>,
    pub refresh_fails: bool,
    pub code_verifier: Option<String>,
    /// Like Keycloak's "Revoke Refresh Token": every refresh token works once.
    pub rotate_refresh_tokens: bool,
    pub used_refresh_tokens: Vec<String>,
    pub refresh_calls: usize,
    /// Latency of the token endpoint, so that concurrent requests overlap.
    pub token_delay: std::time::Duration,
}

pub(crate) struct Idp {
    pub server: MockServer,
    pub state: Arc<Mutex<IdpState>>,
}

struct TokenResponder {
    issuer: String,
    key: CoreRsaPrivateSigningKey,
    state: Arc<Mutex<IdpState>>,
}

impl TokenResponder {
    fn id_token(&self, nonce: Option<String>, roles: Vec<String>) -> String {
        let name = EndUserName::new("Peter Test".to_string());
        let mut claims = TestClaims::new(
            IssuerUrl::new(self.issuer.clone()).unwrap(),
            vec![Audience::new(CLIENT_ID.to_string())],
            chrono::Utc::now() + chrono::Duration::hours(1),
            chrono::Utc::now(),
            StandardClaims::new(SubjectIdentifier::new("sub-1".to_string()))
                .set_name(Some(name.into())),
            RolesClaim { roles },
        );
        if let Some(nonce) = nonce {
            claims = claims.set_nonce(Some(Nonce::new(nonce)));
        }
        TestIdToken::new(
            claims,
            &self.key,
            CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
            None,
            None,
        )
        .unwrap()
        .to_string()
    }
}

impl Respond for TokenResponder {
    fn respond(&self, request: &MockRequest) -> ResponseTemplate {
        let form: Vec<(String, String)> = url::form_urlencoded::parse(&request.body)
            .into_owned()
            .collect();
        let field = |name: &str| form.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
        let mut state = self.state.lock().unwrap();
        let (id_token, refresh) = match field("grant_type").as_deref() {
            Some("authorization_code") if field("code").as_deref() == Some("good-code") => {
                state.code_verifier = field("code_verifier");
                (
                    self.id_token(state.nonce.clone(), state.roles.clone()),
                    "refresh-1".to_string(),
                )
            }
            Some("refresh_token") if !state.refresh_fails => {
                state.refresh_calls += 1;
                let presented = field("refresh_token").unwrap_or_default();
                if state.rotate_refresh_tokens && state.used_refresh_tokens.contains(&presented) {
                    return ResponseTemplate::new(400)
                        .set_body_json(serde_json::json!({"error": "invalid_grant"}));
                }
                state.used_refresh_tokens.push(presented);
                let refresh = format!("refresh-{}", state.refresh_calls + 1);
                (self.id_token(None, state.roles.clone()), refresh)
            }
            _ => {
                return ResponseTemplate::new(400)
                    .set_body_json(serde_json::json!({"error": "invalid_grant"}));
            }
        };
        ResponseTemplate::new(200)
            .set_delay(state.token_delay)
            .set_body_json(serde_json::json!({
                "access_token": "access",
                "token_type": "Bearer",
                "expires_in": 300,
                "refresh_token": refresh,
                "id_token": id_token,
            }))
    }
}

impl Idp {
    pub(crate) async fn start() -> Self {
        let server = MockServer::start().await;
        let issuer = server.uri();
        let key = CoreRsaPrivateSigningKey::from_pem(
            &SIGNING_KEY_PEM,
            Some(JsonWebKeyId::new("k1".into())),
        )
        .unwrap();
        let jwks = CoreJsonWebKeySet::new(vec![key.as_verification_key()]);
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issuer": issuer,
                "authorization_endpoint": format!("{issuer}/auth"),
                "token_endpoint": format!("{issuer}/token"),
                "jwks_uri": format!("{issuer}/jwks"),
                "end_session_endpoint": format!("{issuer}/logout"),
                "response_types_supported": ["code"],
                "subject_types_supported": ["public"],
                "id_token_signing_alg_values_supported": ["RS256"],
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::to_value(&jwks).unwrap()),
            )
            .mount(&server)
            .await;
        let state = Arc::new(Mutex::new(IdpState {
            nonce: None,
            roles: vec!["board-member".into()],
            refresh_fails: false,
            code_verifier: None,
            rotate_refresh_tokens: false,
            used_refresh_tokens: Vec::new(),
            refresh_calls: 0,
            token_delay: std::time::Duration::ZERO,
        }));
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(TokenResponder {
                issuer,
                key,
                state: state.clone(),
            })
            .mount(&server)
            .await;
        Self { server, state }
    }

    pub(crate) fn set(&self, f: impl FnOnce(&mut IdpState)) {
        f(&mut self.state.lock().unwrap());
    }
}

pub(crate) struct TestApp {
    pub router: Router,
    pub pool: PgPool,
    pub idp: Idp,
    _dir: tempfile::TempDir,
}

/// Routes of the plugins `(name, hosts)`, an entry without hosts being the default; `loaded` are
/// the plugins that loaded.
pub(crate) fn forge_routes(plugins: &[(&str, &[&str])], loaded: &[&str]) -> ForgeRoutes {
    let configs = plugins
        .iter()
        .map(|(name, hosts)| {
            let mut config = PluginConfig::new(format!("/plugins/{name}.wasm"));
            config.hosts = hosts
                .iter()
                .map(|host| HostRoute::Host((*host).to_string()))
                .collect();
            ((*name).to_string(), config)
        })
        .collect();
    let table = RoutingTable::from_plugins(&configs).unwrap();
    ForgeRoutes::new(table, loaded.iter().map(|name| (*name).to_string()))
}

/// Routes with `plugin` as the loaded default plugin: it serves every HTTPS repository.
pub(crate) fn default_forge(plugin: &str) -> ForgeRoutes {
    forge_routes(&[(plugin, &[])], &[plugin])
}

impl TestApp {
    pub(crate) async fn new() -> Self {
        Self::with_forges(default_forge("forgejo")).await
    }

    pub(crate) async fn with_forges(forges: ForgeRoutes) -> Self {
        Self::with_egress(forges, None, None).await
    }

    pub(crate) async fn with_egress(
        forges: ForgeRoutes,
        flows: Option<Arc<dyn crate::egress::FlowSource>>,
        services: Option<Arc<dyn crate::egress::ServiceLookup>>,
    ) -> Self {
        Self::build(forges, flows, services, false).await
    }

    /// An app that shows the hidden transcript blocks (`web.showHiddenBlocks`).
    pub(crate) async fn with_hidden_blocks() -> Self {
        Self::build(default_forge("forgejo"), None, None, true).await
    }

    async fn build(
        forges: ForgeRoutes,
        flows: Option<Arc<dyn crate::egress::FlowSource>>,
        services: Option<Arc<dyn crate::egress::ServiceLookup>>,
        show_hidden_blocks: bool,
    ) -> Self {
        let pool = df_test_postgres::migrated_pool(&df_db::MIGRATOR).await;
        let idp = Idp::start().await;
        let dir = tempfile::tempdir().unwrap();
        let secret_file = dir.path().join("secret");
        std::fs::write(&secret_file, "s3cret\n").unwrap();
        let cfg = AuthConfig {
            issuer_url: idp.server.uri(),
            client_id: CLIENT_ID.into(),
            client_secret_file: secret_file,
            public_url: "https://factory.test".into(),
            roles_claim: "roles".into(),
            extra_scopes: Vec::new(),
        };
        let state = WebState {
            notifier: Arc::new(df_db::Notifier::start(&pool).await.unwrap()),
            oidc: Arc::new(OidcClient::new(&cfg).unwrap()),
            pool: pool.clone(),
            forges,
            flows,
            services,
            show_hidden_blocks,
            approval_types: df_db::requests::ApprovalTypes::default().with(Arc::new(FakeApproval)),
        };
        Self {
            router: router(state),
            pool,
            idp,
            _dir: dir,
        }
    }

    /// The approval types the app was built with.
    pub(crate) fn approval_types() -> df_db::requests::ApprovalTypes {
        df_db::requests::ApprovalTypes::default().with(Arc::new(FakeApproval))
    }

    /// A logged-in `board-member` session: the `Cookie` header value.
    pub(crate) async fn login(&self) -> String {
        crate::auth::testing::seed_session(&self.pool, &["board-member"], 3600, Some("refresh-0"))
            .await
            .0
    }

    pub(crate) async fn send(&self, request: Request<Body>) -> Response<Body> {
        self.router.clone().oneshot(request).await.unwrap()
    }

    pub(crate) async fn get(&self, uri: &str, cookie: &str) -> Response<Body> {
        self.send(
            Request::get(uri)
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    /// A `GET` as htmx sends it (with the `HX-Request` header), for fragments.
    pub(crate) async fn get_htmx(&self, uri: &str, cookie: &str) -> Response<Body> {
        self.send(
            Request::get(uri)
                .header(header::COOKIE, cookie)
                .header("HX-Request", "true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    /// A form post as htmx sends it (with the `HX-Request` header).
    pub(crate) async fn post(&self, uri: &str, body: &str, cookie: &str) -> Response<Body> {
        self.send(post_request(uri, body, cookie, true)).await
    }

    /// A `PATCH` with a form body as htmx sends it.
    pub(crate) async fn patch(&self, uri: &str, body: &str, cookie: &str) -> Response<Body> {
        let mut request = post_request(uri, body, cookie, true);
        *request.method_mut() = axum::http::Method::PATCH;
        self.send(request).await
    }

    /// A `DELETE` as htmx sends it.
    pub(crate) async fn delete(&self, uri: &str, cookie: &str) -> Response<Body> {
        let mut request = post_request(uri, "", cookie, true);
        *request.method_mut() = axum::http::Method::DELETE;
        self.send(request).await
    }
}

pub(crate) fn post_request(uri: &str, body: &str, cookie: &str, htmx: bool) -> Request<Body> {
    let mut request = Request::post(uri)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, cookie);
    if htmx {
        request = request.header("HX-Request", "true");
    }
    request.body(Body::from(body.to_string())).unwrap()
}

pub(crate) async fn text(response: Response<Body>) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8_lossy(&bytes).into_owned()
}

pub(crate) fn header_str<'a>(response: &'a Response<Body>, name: &str) -> &'a str {
    response
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}

/// Url-encodes `key=value` pairs.
pub(crate) fn form(pairs: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish()
}

/// A project and a persona to create tasks with.
pub(crate) struct Seed {
    pub project: uuid::Uuid,
    pub persona: uuid::Uuid,
}

pub(crate) fn actor() -> (df_core::Principal, Vec<String>) {
    (
        df_core::Principal::human(df_core::UserId::new(), "Peter"),
        vec!["board-member".to_string()],
    )
}

pub(crate) async fn seed(pool: &PgPool) -> Seed {
    use df_db::catalog::{Actor, PersonaInput, ProjectInput, save_persona, save_project};
    let (principal, roles) = actor();
    let actor = Actor {
        principal: &principal,
        roles: &roles,
    };
    let project = save_project(
        pool,
        actor,
        None,
        &ProjectInput {
            key: "DF".into(),
            name: "Dark Factory".into(),
            repo_url: "https://git.example.org/peter/dark-factory.git".into(),
            issue_tracker_url: None,
            image: "img:latest".into(),
            base_branch: "main".into(),
            adapter_command: vec![],
            forge_credential_id: None,
        },
    )
    .await
    .unwrap();
    let persona = save_persona(
        pool,
        actor,
        None,
        &PersonaInput {
            name: "Linus".into(),
            system_prompt: "Be Linus.".into(),
            harness: "claude-code".into(),
            model: None,
            harness_credential_id: None,
        },
    )
    .await
    .unwrap();
    Seed {
        project: project.id,
        persona: persona.id,
    }
}

pub(crate) async fn new_task(pool: &PgPool, seed: &Seed, title: &str) -> df_db::TaskRow {
    df_db::create_task(
        pool,
        df_db::NewTask {
            project_id: df_core::ProjectId::from_uuid(seed.project),
            persona_id: df_core::PersonaId::from_uuid(seed.persona),
            title: Some(title.into()),
            description: String::new(),
            issue_number: None,
            created_by: None,
        },
    )
    .await
    .unwrap()
    .0
}

pub(crate) async fn insert_run(
    pool: &PgPool,
    task: df_core::TaskId,
    number: i64,
    state: &str,
) -> uuid::Uuid {
    let id = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO runs (id, task_id, number, state) VALUES ($1, $2, $3, $4)")
        .bind(id)
        .bind(task.as_uuid())
        .bind(number)
        .bind(state)
        .execute(pool)
        .await
        .unwrap();
    id
}

/// Stores a transcript event (`kind` is the `snake_case` name of the event's oneof field) and
/// publishes the notification the writer publishes. Returns the event id.
pub(crate) async fn add_event(
    pool: &PgPool,
    task: df_core::TaskId,
    run: uuid::Uuid,
    seq: i64,
    turn: &str,
    body: serde_json::Value,
) -> i64 {
    let (name, _) = body.as_object().unwrap().iter().next().unwrap();
    let kind: String = name
        .chars()
        .flat_map(|c| {
            if c.is_ascii_uppercase() {
                vec!['_', c.to_ascii_lowercase()]
            } else {
                vec![c]
            }
        })
        .collect();
    let mut payload = serde_json::json!({"schemaVersion": 1, "turnId": turn});
    payload
        .as_object_mut()
        .unwrap()
        .extend(body.as_object().unwrap().clone());
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO transcript_events (task_id, run_id, seq, occurred_at, kind, payload)
         VALUES ($1, $2, $3, now(), $4, $5) RETURNING id",
    )
    .bind(task.as_uuid())
    .bind(run)
    .bind(seq)
    .bind(kind)
    .bind(payload)
    .fetch_one(pool)
    .await
    .unwrap();
    df_db::publish(
        pool,
        &df_db::Notification::TranscriptEvent { task_id: task, id },
    )
    .await
    .unwrap();
    id
}

pub(crate) fn chunk(message: &str, text: &str) -> serde_json::Value {
    serde_json::json!({"agentMessageChunk": {"messageId": message, "content": {"text": {"text": text}}}})
}

/// One server-sent event.
#[derive(Debug)]
pub(crate) struct Frame {
    pub id: Option<String>,
    pub data: String,
}

/// The event in `raw` (lines up to the blank line); `None` without data, such as a keep-alive.
fn parse_frame(raw: &str) -> Option<Frame> {
    let mut id = None;
    let mut data: Option<String> = None;
    for line in raw.lines() {
        if let Some(value) = line.strip_prefix("id: ") {
            id = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("data: ") {
            match &mut data {
                Some(data) => {
                    data.push('\n');
                    data.push_str(value);
                }
                None => data = Some(value.to_string()),
            }
        }
    }
    data.map(|data| Frame { id, data })
}

/// Reads the events of an SSE response.
pub(crate) struct SseReader {
    body: axum::body::BodyDataStream,
    buf: String,
}

impl SseReader {
    pub(crate) fn new(response: Response<Body>) -> Self {
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        Self {
            body: response.into_body().into_data_stream(),
            buf: String::new(),
        }
    }

    /// The first complete event in the buffer, if any; keep-alives are skipped.
    fn take_frame(&mut self) -> Option<Frame> {
        while let Some(end) = self.buf.find("\n\n") {
            let raw: String = self.buf.drain(..end + 2).collect();
            if let Some(frame) = parse_frame(&raw) {
                return Some(frame);
            }
        }
        None
    }

    /// The next event; panics if none arrives within `timeout`.
    pub(crate) async fn next_within(&mut self, timeout: std::time::Duration) -> Frame {
        use futures::StreamExt;
        tokio::time::timeout(timeout, async {
            loop {
                if let Some(frame) = self.take_frame() {
                    return frame;
                }
                let chunk = self
                    .body
                    .next()
                    .await
                    .expect("stream ended")
                    .expect("body error");
                self.buf.push_str(&String::from_utf8_lossy(&chunk));
            }
        })
        .await
        .expect("timed out waiting for an event")
    }

    pub(crate) async fn next(&mut self) -> Frame {
        self.next_within(std::time::Duration::from_secs(5)).await
    }

    /// Whether the server ends the stream within `timeout`; events before the end are skipped.
    pub(crate) async fn ends_within(&mut self, timeout: std::time::Duration) -> bool {
        use futures::StreamExt;
        tokio::time::timeout(timeout, async { while self.body.next().await.is_some() {} })
            .await
            .is_ok()
    }
}

/// The opening tag of the element at the first occurrence of `marker` (text or attribute): from
/// the `<` before it to the next `>`.
pub(crate) fn tag_around<'a>(html: &'a str, marker: &str) -> &'a str {
    let at = html
        .find(marker)
        .unwrap_or_else(|| panic!("{marker}: {html}"));
    let start = html[..at].rfind('<').unwrap();
    &html[start..=(start + html[start..].find('>').unwrap())]
}

/// An approval type for tests: scopes task and project, applying it does nothing.
struct FakeApproval;

#[async_trait::async_trait]
impl df_db::requests::ApprovalType for FakeApproval {
    fn name(&self) -> &'static str {
        "fake-approval"
    }
    fn validate(
        &self,
        _task: &df_db::TaskRow,
        _payload: &serde_json::Value,
    ) -> Result<(), df_db::requests::RequestError> {
        Ok(())
    }
    fn fingerprint(&self, payload: &serde_json::Value) -> String {
        payload.to_string()
    }
    fn scopes(&self) -> &'static [df_db::requests::Scope] {
        &[
            df_db::requests::Scope::Task,
            df_db::requests::Scope::Project,
        ]
    }
    async fn apply(
        &self,
        _tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        _request: &df_db::requests::RequestRow,
        _scope: df_db::requests::Scope,
    ) -> Result<(), df_db::requests::RequestError> {
        Ok(())
    }
    fn describe(&self, _request: &df_db::requests::RequestRow) -> String {
        "Fake approval".to_string()
    }
}
