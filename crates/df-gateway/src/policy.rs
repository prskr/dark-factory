//! The gateway's view of the policy snapshot and the pure allow/deny decision.

use std::collections::HashMap;
use std::net::IpAddr;

use df_proto::gateway::v1::{
    ForgeCredential, GitRoute, InferenceRoute, McpServer, PolicyUpdate, ProviderCredential,
    PushPolicy, SandboxEntry,
};

/// Reason of a denied request, as shown in the egress log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denial {
    /// No snapshot received yet (fail closed).
    NoSnapshot,
    /// The source IP is not an active sandbox.
    UnknownSource,
    /// The destination is an IP literal.
    IpLiteral,
    /// No allowlist entry of the run matches host and port.
    NotAllowlisted,
}

impl Denial {
    /// The reason string of the flow event.
    pub const fn reason(self) -> &'static str {
        match self {
            Self::NoSnapshot => "no-snapshot",
            Self::UnknownSource => "unknown-source",
            Self::IpLiteral => "ip-literal",
            Self::NotAllowlisted => "not-allowlisted",
        }
    }

    /// HTTP status of the proxy's answer: 503 while the gateway has no policy, 403 otherwise.
    pub const fn status(self) -> u16 {
        match self {
            Self::NoSnapshot => 503,
            _ => 403,
        }
    }
}

/// What the proxy is asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request<'a> {
    /// `CONNECT host:port`.
    Connect { host: &'a str, port: u16 },
    /// Absolute-form plain HTTP request.
    Http { host: &'a str, port: u16 },
}

impl Request<'_> {
    const fn host(&self) -> &str {
        match self {
            Self::Connect { host, .. } | Self::Http { host, .. } => host,
        }
    }

    const fn port(&self) -> u16 {
        match self {
            Self::Connect { port, .. } | Self::Http { port, .. } => *port,
        }
    }

    /// Whether an allowlist entry can allow the request: `CONNECT` on any port, plain HTTP only
    /// on port 80.
    pub const fn allowlistable(&self) -> bool {
        match self {
            Self::Connect { .. } => true,
            Self::Http { port, .. } => *port == 80,
        }
    }
}

/// An allowlist entry of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRule {
    /// Lower case hostname or `*.suffix`.
    pub pattern: String,
    pub port: u16,
    pub scope: String,
    pub entry_id: String,
}

impl HostRule {
    fn matches_host(&self, host: &str) -> bool {
        df_core::host::pattern_matches(&self.pattern, host)
    }
}

/// An active sandbox and what it may reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sandbox {
    pub run_id: String,
    pub task_key: String,
    pub run_number: u32,
    pub project_key: String,
    pub persona_id: String,
    pub hosts: Vec<HostRule>,
}

/// The sandbox registry with the allowlists of the latest snapshot.
/// Not `Eq`: the inference routes of the snapshot are messages with nested messages.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Policy {
    pub version: u64,
    sandboxes: HashMap<IpAddr, Sandbox>,
    git_routes: HashMap<IpAddr, GitRoute>,
    forge_credentials: HashMap<String, ForgeCredential>,
    push_policies: HashMap<String, PushPolicy>,
    exceptions: HashMap<IpAddr, Vec<String>>,
    inference_routes: HashMap<IpAddr, InferenceRoute>,
    provider_credentials: HashMap<String, ProviderCredential>,
    mcp_servers: HashMap<String, McpServer>,
    mcp_server_ids: HashMap<IpAddr, Vec<String>>,
}

impl Policy {
    pub fn new(version: u64, sandboxes: impl IntoIterator<Item = (IpAddr, Sandbox)>) -> Self {
        Self {
            version,
            sandboxes: sandboxes.into_iter().collect(),
            git_routes: HashMap::new(),
            forge_credentials: HashMap::new(),
            push_policies: HashMap::new(),
            exceptions: HashMap::new(),
            inference_routes: HashMap::new(),
            provider_credentials: HashMap::new(),
            mcp_servers: HashMap::new(),
            mcp_server_ids: HashMap::new(),
        }
    }

    /// Adds the repositories of the sandboxes (by pod IP) and the forge credentials they refer to.
    #[must_use]
    pub fn with_git(
        mut self,
        routes: impl IntoIterator<Item = (IpAddr, GitRoute)>,
        credentials: impl IntoIterator<Item = (String, ForgeCredential)>,
    ) -> Self {
        self.git_routes = routes.into_iter().collect();
        self.forge_credentials = credentials.into_iter().collect();
        self
    }

    /// Adds the push policies of the projects (by project key) and the exceptions of the
    /// sandboxes (by pod IP).
    #[must_use]
    pub fn with_push_checks(
        mut self,
        policies: impl IntoIterator<Item = (String, PushPolicy)>,
        exceptions: impl IntoIterator<Item = (IpAddr, Vec<String>)>,
    ) -> Self {
        self.push_policies = policies.into_iter().collect();
        self.exceptions = exceptions.into_iter().collect();
        self
    }

    /// Adds the model providers of the sandboxes (by pod IP) and the credentials they refer to.
    #[must_use]
    pub fn with_inference(
        mut self,
        routes: impl IntoIterator<Item = (IpAddr, InferenceRoute)>,
        credentials: impl IntoIterator<Item = (String, ProviderCredential)>,
    ) -> Self {
        self.inference_routes = routes.into_iter().collect();
        self.provider_credentials = credentials.into_iter().collect();
        self
    }

    /// Entries whose `pod_ip` does not parse are skipped: they can never match a connection.
    pub fn from_update(update: PolicyUpdate) -> Self {
        let snapshot = update.snapshot.unwrap_or_default();
        let mut routes = Vec::new();
        let mut inference = Vec::new();
        let mut exceptions = Vec::new();
        let mut mcp_ids = Vec::new();
        let sandboxes: Vec<_> = snapshot
            .sandboxes
            .into_iter()
            .filter_map(|mut entry| {
                let route = entry.git.take();
                let model = entry.inference.take();
                let excepted = std::mem::take(&mut entry.exceptions);
                let servers = std::mem::take(&mut entry.mcp_server_ids);
                let (ip, sandbox) = sandbox_from_entry(entry)?;
                routes.extend(route.map(|route| (ip, route)));
                inference.extend(model.map(|route| (ip, route)));
                exceptions.push((ip, excepted));
                mcp_ids.push((ip, servers));
                Some((ip, sandbox))
            })
            .collect();
        Self::new(update.version, sandboxes)
            .with_git(routes, snapshot.forge_credentials)
            .with_push_checks(snapshot.push_policies, exceptions)
            .with_inference(inference, snapshot.provider_credentials)
            .with_mcp(snapshot.mcp_servers, mcp_ids)
    }

    /// Adds the MCP servers (by server id) and the ids of the servers enabled for each sandbox
    /// (by pod IP). Their credentials are among the provider credentials
    /// ([`Self::with_inference`]).
    #[must_use]
    pub fn with_mcp(
        mut self,
        servers: impl IntoIterator<Item = (String, McpServer)>,
        enabled: impl IntoIterator<Item = (IpAddr, Vec<String>)>,
    ) -> Self {
        self.mcp_servers = servers.into_iter().collect();
        self.mcp_server_ids = enabled.into_iter().collect();
        self
    }

    /// The MCP servers enabled for the sandbox at `ip`, by name. A listed id the snapshot does
    /// not carry is skipped.
    pub fn mcp_servers(&self, ip: IpAddr) -> Vec<&McpServer> {
        let mut servers: Vec<&McpServer> = self
            .mcp_server_ids
            .get(&ip)
            .into_iter()
            .flatten()
            .filter_map(|id| self.mcp_servers.get(id))
            .collect();
        servers.sort_by(|a, b| a.name.cmp(&b.name));
        servers
    }

    /// The MCP server with this id, whichever runs have it enabled.
    pub fn mcp_server_by_id(&self, id: &str) -> Option<&McpServer> {
        self.mcp_servers.get(id)
    }

    /// The MCP server `name` enabled for the sandbox at `ip`.
    pub fn mcp_server(&self, ip: IpAddr, name: &str) -> Option<&McpServer> {
        self.mcp_servers(ip)
            .into_iter()
            .find(|server| server.name == name)
    }

    /// What the aggregated tool list of the sandbox at `ip` depends on: its enabled servers with
    /// their configuration versions. Changes when a server is enabled, disabled or edited.
    pub fn mcp_signature(&self, ip: IpAddr) -> Vec<(String, u64)> {
        self.mcp_servers(ip)
            .into_iter()
            .map(|server| (server.id.clone(), server.config_version))
            .collect()
    }

    /// Whether the run `run_id` still has an entry in the snapshot.
    pub fn has_run(&self, run_id: &str) -> bool {
        self.sandboxes
            .values()
            .any(|sandbox| sandbox.run_id == run_id)
    }

    /// Whether the run `run_id` has the server `id` enabled with this configuration version.
    pub fn enables_mcp_server(&self, run_id: &str, id: &str, config_version: u64) -> bool {
        self.sandboxes.iter().any(|(ip, sandbox)| {
            sandbox.run_id == run_id
                && self
                    .mcp_servers(*ip)
                    .iter()
                    .any(|server| server.id == id && server.config_version == config_version)
        })
    }

    pub fn sandbox(&self, ip: IpAddr) -> Option<&Sandbox> {
        self.sandboxes.get(&ip)
    }

    /// Every active sandbox.
    pub fn sandboxes(&self) -> impl Iterator<Item = &Sandbox> {
        self.sandboxes.values()
    }

    /// The repository of the sandbox at `ip`, if its run has one.
    pub fn git_route(&self, ip: IpAddr) -> Option<&GitRoute> {
        self.git_routes.get(&ip)
    }

    /// The credential a [`GitRoute::credential_ref`] names.
    pub fn forge_credential(&self, reference: &str) -> Option<&ForgeCredential> {
        self.forge_credentials.get(reference)
    }

    /// The model provider route of the sandbox at `ip`, if its run has one.
    pub fn inference_route(&self, ip: IpAddr) -> Option<&InferenceRoute> {
        self.inference_routes.get(&ip)
    }

    /// The credential an [`InferenceRoute::credential_ref`] names.
    pub fn provider_credential(&self, reference: &str) -> Option<&ProviderCredential> {
        self.provider_credentials.get(reference)
    }

    /// The push policy of the project `project_key`.
    pub fn push_policy(&self, project_key: &str) -> Option<&PushPolicy> {
        self.push_policies.get(project_key)
    }

    /// Fingerprints of the push check findings approved for the task of the sandbox at `ip`.
    pub fn exceptions(&self, ip: IpAddr) -> &[String] {
        self.exceptions.get(&ip).map_or(&[], Vec::as_slice)
    }
}

fn sandbox_from_entry(entry: SandboxEntry) -> Option<(IpAddr, Sandbox)> {
    let ip = entry.pod_ip.parse().ok()?;
    let hosts = entry
        .hosts
        .into_iter()
        .filter_map(|rule| {
            Some(HostRule {
                pattern: df_core::host::normalize_host(&rule.pattern),
                port: u16::try_from(rule.port).ok().filter(|port| *port != 0)?,
                scope: rule.scope,
                entry_id: rule.entry_id,
            })
        })
        .collect();
    Some((
        ip,
        Sandbox {
            run_id: entry.run_id,
            task_key: entry.task_key,
            run_number: entry.run_number,
            project_key: entry.project_key,
            persona_id: entry.persona_id,
            hosts,
        },
    ))
}

/// The outcome of [`decide`]: the identified sandbox (if any) and the verdict.
#[derive(Debug, PartialEq, Eq)]
pub struct Decision<'a> {
    pub sandbox: Option<&'a Sandbox>,
    pub verdict: Result<&'a HostRule, Denial>,
}

/// Whether the sandbox at `source` may make `request`. `host` of `request` must be normalized
/// ([`df_core::host::normalize_host`]).
pub fn decide<'a>(
    policy: Option<&'a Policy>,
    source: IpAddr,
    request: Request<'_>,
) -> Decision<'a> {
    let Some(policy) = policy else {
        return Decision {
            sandbox: None,
            verdict: Err(Denial::NoSnapshot),
        };
    };
    let Some(sandbox) = policy.sandbox(source) else {
        return Decision {
            sandbox: None,
            verdict: Err(Denial::UnknownSource),
        };
    };
    let verdict = if request.host().parse::<IpAddr>().is_ok() {
        Err(Denial::IpLiteral)
    } else if !request.allowlistable() {
        Err(Denial::NotAllowlisted)
    } else {
        sandbox
            .hosts
            .iter()
            .find(|rule| rule.port == request.port() && rule.matches_host(request.host()))
            .ok_or(Denial::NotAllowlisted)
    };
    Decision {
        sandbox: Some(sandbox),
        verdict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POD: &str = "10.1.0.7";

    fn rule(pattern: &str, port: u16) -> HostRule {
        HostRule {
            pattern: pattern.into(),
            port,
            scope: "instance".into(),
            entry_id: format!("e-{pattern}"),
        }
    }

    fn policy(hosts: Vec<HostRule>) -> Policy {
        Policy::new(
            1,
            [(
                POD.parse().unwrap(),
                Sandbox {
                    run_id: "run-1".into(),
                    task_key: "DF-1".into(),
                    run_number: 2,
                    project_key: "DF".into(),
                    persona_id: "p".into(),
                    hosts,
                },
            )],
        )
    }

    fn verdict(policy: &Policy, request: Request<'_>) -> Result<String, Denial> {
        decide(Some(policy), POD.parse().unwrap(), request)
            .verdict
            .map(|rule| rule.pattern.clone())
    }

    fn connect(host: &str, port: u16) -> Request<'_> {
        Request::Connect { host, port }
    }

    #[test]
    fn unknown_source_is_denied_with_403() {
        let policy = policy(vec![rule("api.anthropic.com", 443)]);
        let decision = decide(
            Some(&policy),
            "10.9.9.9".parse().unwrap(),
            connect("api.anthropic.com", 443),
        );
        assert_eq!(decision.verdict, Err(Denial::UnknownSource));
        assert!(decision.sandbox.is_none());
        assert_eq!(Denial::UnknownSource.status(), 403);
    }

    #[test]
    fn without_a_snapshot_everything_is_denied_with_503() {
        let decision = decide(
            None,
            POD.parse().unwrap(),
            connect("api.anthropic.com", 443),
        );
        assert_eq!(decision.verdict, Err(Denial::NoSnapshot));
        assert_eq!(Denial::NoSnapshot.status(), 503);
        assert_eq!(Denial::NoSnapshot.reason(), "no-snapshot");
    }

    #[test]
    fn exact_and_wildcard_hosts_match() {
        let policy = policy(vec![
            rule("api.anthropic.com", 443),
            rule("*.crates.io", 443),
        ]);
        assert_eq!(
            verdict(&policy, connect("api.anthropic.com", 443)),
            Ok("api.anthropic.com".into())
        );
        assert_eq!(
            verdict(&policy, connect("static.crates.io", 443)),
            Ok("*.crates.io".into())
        );
        assert_eq!(
            verdict(&policy, connect("a.b.crates.io", 443)),
            Ok("*.crates.io".into())
        );
        for denied in [
            "crates.io",
            "evilcrates.io",
            "crates.io.evil.com",
            "example.com",
        ] {
            assert_eq!(
                verdict(&policy, connect(denied, 443)),
                Err(Denial::NotAllowlisted),
                "{denied}"
            );
        }
        assert_eq!(Denial::NotAllowlisted.status(), 403);
    }

    #[test]
    fn the_port_must_match_the_entry() {
        let policy = policy(vec![rule("api.anthropic.com", 443)]);
        assert_eq!(
            verdict(&policy, connect("api.anthropic.com", 8443)),
            Err(Denial::NotAllowlisted)
        );
    }

    #[test]
    fn ip_literals_are_denied_even_when_listed() {
        let policy = policy(vec![rule("1.1.1.1", 443), rule("*.io", 443)]);
        for host in ["1.1.1.1", "::1", "10.0.0.5"] {
            assert_eq!(
                verdict(&policy, connect(host, 443)),
                Err(Denial::IpLiteral),
                "{host}"
            );
        }
    }

    #[test]
    fn plain_http_needs_an_explicit_port_80_entry() {
        let http = |host| Request::Http { host, port: 80 };
        let policy = policy(vec![rule("deb.debian.org", 443), rule("pypi.org", 80)]);
        assert_eq!(
            verdict(&policy, http("deb.debian.org")),
            Err(Denial::NotAllowlisted)
        );
        assert_eq!(verdict(&policy, http("pypi.org")), Ok("pypi.org".into()));
        // Not forwarded to other ports even if an entry names that port.
        let policy = self::policy(vec![rule("pypi.org", 8080)]);
        assert_eq!(
            verdict(
                &policy,
                Request::Http {
                    host: "pypi.org",
                    port: 8080
                }
            ),
            Err(Denial::NotAllowlisted)
        );
    }

    #[test]
    fn snapshot_conversion_carries_the_run_number() {
        use df_proto::gateway::v1::Snapshot;
        let policy = Policy::from_update(PolicyUpdate {
            version: 1,
            snapshot: Some(Snapshot {
                sandboxes: vec![SandboxEntry {
                    pod_ip: POD.into(),
                    run_number: 2,
                    ..Default::default()
                }],
                ..Default::default()
            }),
        });
        assert_eq!(policy.sandbox(POD.parse().unwrap()).unwrap().run_number, 2);
    }

    #[test]
    fn snapshot_conversion_keeps_git_routes_and_forge_credentials() {
        use df_proto::gateway::v1::Snapshot;
        let route = GitRoute {
            credential_ref: "forge".into(),
            ..Default::default()
        };
        let credential = ForgeCredential {
            username: "bot".into(),
            password: "secret".into(),
        };
        let policy = Policy::from_update(PolicyUpdate {
            version: 1,
            snapshot: Some(Snapshot {
                sandboxes: vec![SandboxEntry {
                    pod_ip: POD.into(),
                    git: Some(route.clone()),
                    ..Default::default()
                }],
                forge_credentials: [("forge".to_string(), credential.clone())].into(),
                ..Default::default()
            }),
        });
        assert_eq!(policy.git_route(POD.parse().unwrap()), Some(&route));
        assert_eq!(policy.forge_credential("forge"), Some(&credential));
        assert_eq!(policy.git_route("10.9.9.9".parse().unwrap()), None);
    }

    #[test]
    fn snapshot_conversion_keeps_push_policies_and_exceptions() {
        use df_proto::gateway::v1::{PushPolicy, Snapshot};
        let push_policy = PushPolicy {
            protected_globs: vec!["deploy/**".into()],
            max_blob_bytes: 1024,
            ..Default::default()
        };
        let policy = Policy::from_update(PolicyUpdate {
            version: 1,
            snapshot: Some(Snapshot {
                sandboxes: vec![SandboxEntry {
                    pod_ip: POD.into(),
                    project_key: "DF".into(),
                    exceptions: vec!["path:deploy/values.yaml".into()],
                    ..Default::default()
                }],
                push_policies: [("DF".to_string(), push_policy.clone())].into(),
                ..Default::default()
            }),
        });
        assert_eq!(policy.push_policy("DF"), Some(&push_policy));
        assert_eq!(policy.push_policy("OTHER"), None);
        assert_eq!(
            policy.exceptions(POD.parse().unwrap()),
            ["path:deploy/values.yaml"]
        );
        assert!(policy.exceptions("10.9.9.9".parse().unwrap()).is_empty());
    }

    #[test]
    fn snapshot_conversion_normalizes_and_skips_unusable_entries() {
        use df_proto::gateway::v1::{HostRule as ProtoRule, Snapshot};
        let entry = |ip: &str, pattern: &str, port: u32| SandboxEntry {
            pod_ip: ip.into(),
            run_id: "r".into(),
            hosts: vec![ProtoRule {
                pattern: pattern.into(),
                port,
                ..Default::default()
            }],
            ..Default::default()
        };
        let policy = Policy::from_update(PolicyUpdate {
            version: 7,
            snapshot: Some(Snapshot {
                sandboxes: vec![
                    entry("10.1.0.7", "API.Anthropic.com.", 443),
                    entry("not-an-ip", "x.com", 443),
                    entry("10.1.0.8", "x.com", 0),
                ],
                ..Default::default()
            }),
        });
        assert_eq!(policy.version, 7);
        let first = policy.sandbox(POD.parse().unwrap()).unwrap();
        assert_eq!(first.hosts[0].pattern, "api.anthropic.com");
        // Valid IP, but its only rule has no usable port.
        assert!(
            policy
                .sandbox("10.1.0.8".parse().unwrap())
                .unwrap()
                .hosts
                .is_empty()
        );
    }

    #[test]
    fn mcp_servers_of_a_sandbox_come_from_the_snapshot_by_name() {
        use df_proto::gateway::v1::Snapshot;
        let server = |id: &str, name: &str, version: u64| McpServer {
            id: id.into(),
            name: name.into(),
            config_version: version,
            credential_ref: format!("c-{id}"),
            ..Default::default()
        };
        let entry = SandboxEntry {
            pod_ip: POD.into(),
            run_id: "run-1".into(),
            project_key: "DF".into(),
            mcp_server_ids: vec!["s2".into(), "s1".into(), "gone".into()],
            ..Default::default()
        };
        let other = SandboxEntry {
            pod_ip: "10.1.0.8".into(),
            run_id: "run-2".into(),
            ..Default::default()
        };
        let update = |version: u64| PolicyUpdate {
            version: 1,
            snapshot: Some(Snapshot {
                sandboxes: vec![entry.clone(), other.clone()],
                mcp_servers: [
                    ("s1".to_owned(), server("s1", "wiki", 1)),
                    ("s2".to_owned(), server("s2", "context7", version)),
                    ("s3".to_owned(), server("s3", "billing", 1)),
                ]
                .into(),
                provider_credentials: [(
                    "c-s2".to_owned(),
                    ProviderCredential {
                        value: "key".into(),
                    },
                )]
                .into(),
                ..Default::default()
            }),
        };
        let policy = Policy::from_update(update(1));
        let ip = POD.parse().unwrap();

        let names: Vec<_> = policy
            .mcp_servers(ip)
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, ["context7", "wiki"]);
        assert!(policy.mcp_servers("10.1.0.8".parse().unwrap()).is_empty());
        assert!(
            policy.mcp_server(ip, "billing").is_none(),
            "not enabled for the run"
        );
        let context7 = policy.mcp_server(ip, "context7").unwrap();
        assert_eq!(
            policy
                .provider_credential(&context7.credential_ref)
                .unwrap()
                .value,
            "key"
        );
        let wiki = policy.mcp_server(ip, "wiki").unwrap();
        assert!(
            policy.provider_credential(&wiki.credential_ref).is_none(),
            "server without credential"
        );
        assert!(policy.has_run("run-1") && !policy.has_run("run-9"));
        assert!(policy.enables_mcp_server("run-1", "s2", 1));
        assert!(!policy.enables_mcp_server("run-1", "s3", 1));
        assert!(!policy.enables_mcp_server("run-2", "s2", 1));

        let edited = Policy::from_update(update(2));
        assert_ne!(policy.mcp_signature(ip), edited.mcp_signature(ip));
        assert_eq!(
            policy.mcp_signature(ip),
            Policy::from_update(update(1)).mcp_signature(ip)
        );
        assert!(!edited.enables_mcp_server("run-1", "s2", 1));
    }
}
