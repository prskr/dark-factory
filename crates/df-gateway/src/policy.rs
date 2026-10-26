//! The gateway's view of the policy snapshot and the pure allow/deny decision.

use std::collections::HashMap;
use std::net::IpAddr;

use df_proto::gateway::v1::{PolicyUpdate, SandboxEntry};

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
        match self.pattern.strip_prefix("*.") {
            // `*.example.com` covers every name below `example.com`, not `example.com` itself.
            Some(suffix) => host
                .strip_suffix(suffix)
                .is_some_and(|label| label.len() > 1 && label.ends_with('.')),
            None => self.pattern == host,
        }
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policy {
    pub version: u64,
    sandboxes: HashMap<IpAddr, Sandbox>,
}

impl Policy {
    pub fn new(version: u64, sandboxes: impl IntoIterator<Item = (IpAddr, Sandbox)>) -> Self {
        Self {
            version,
            sandboxes: sandboxes.into_iter().collect(),
        }
    }

    /// Entries whose `pod_ip` does not parse are skipped: they can never match a connection.
    pub fn from_update(update: PolicyUpdate) -> Self {
        let entries = update.snapshot.unwrap_or_default().sandboxes;
        Self::new(
            update.version,
            entries.into_iter().filter_map(sandbox_from_entry),
        )
    }

    pub fn sandbox(&self, ip: IpAddr) -> Option<&Sandbox> {
        self.sandboxes.get(&ip)
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
    } else {
        let port = match request {
            Request::Connect { port, .. } => port,
            // Plain HTTP only goes to port 80, and only when an entry names port 80.
            Request::Http { port: 80, .. } => 80,
            Request::Http { .. } => return denied(sandbox, Denial::NotAllowlisted),
        };
        sandbox
            .hosts
            .iter()
            .find(|rule| rule.port == port && rule.matches_host(request.host()))
            .ok_or(Denial::NotAllowlisted)
    };
    Decision {
        sandbox: Some(sandbox),
        verdict,
    }
}

fn denied(sandbox: &Sandbox, denial: Denial) -> Decision<'_> {
    Decision {
        sandbox: Some(sandbox),
        verdict: Err(denial),
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
            }),
        });
        assert_eq!(policy.sandbox(POD.parse().unwrap()).unwrap().run_number, 2);
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
}
