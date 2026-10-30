//! Forge routing table: which plugin serves which repository host (design D3). Built from the
//! configuration alone, without Wasm, so the forge router and the web UI share it.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;
use url::Url;

use crate::config::{AuthScheme, AuthSchemeSetting, HostRoute, HostWithApiUrl, PluginConfig};
use crate::forge::ForgeKind;
use crate::host::normalize_host;
use crate::seams::RepoRef;

const HTTPS: &str = "https";
const HTTP: &str = "http";

/// Where forge calls for a host go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// Name of the plugin entry under `plugins`.
    pub plugin: String,
    /// Normalized API base URL; `None` for the default plugin, whose base URL comes from the
    /// repository URL.
    pub api_url: Option<String>,
    /// How the forge credential is sent.
    pub auth_scheme: AuthScheme,
}

/// Normalized host → route, plus the route of the default plugin (the entry without `hosts`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoutingTable {
    hosts: BTreeMap<String, Route>,
    default: Option<Route>,
    allow_plain_http: bool,
}

impl RoutingTable {
    /// Validates the routes of all plugin entries and builds the table.
    pub fn from_plugins(plugins: &BTreeMap<String, PluginConfig>) -> Result<Self, RoutingError> {
        Self::build(plugins, false)
    }

    /// Like [`Self::from_plugins`], but also accepts `http://` `apiUrl`s; for tests against a
    /// local mock server.
    pub fn from_plugins_allowing_plain_http(
        plugins: &BTreeMap<String, PluginConfig>,
    ) -> Result<Self, RoutingError> {
        Self::build(plugins, true)
    }

    fn build(
        plugins: &BTreeMap<String, PluginConfig>,
        allow_plain_http: bool,
    ) -> Result<Self, RoutingError> {
        let defaults: Vec<String> = plugins
            .iter()
            .filter(|(_, config)| config.hosts.is_empty())
            .map(|(name, _)| name.clone())
            .collect();
        if defaults.len() > 1 {
            return Err(RoutingError::SeveralDefaults { plugins: defaults });
        }
        let mut table = Self {
            allow_plain_http,
            ..Self::default()
        };
        for (name, config) in plugins {
            let auth_scheme = known_auth_scheme(name, &config.auth_scheme)?;
            if config.hosts.is_empty() {
                table.default = Some(Route {
                    plugin: name.clone(),
                    api_url: None,
                    auth_scheme,
                });
            }
            for (index, entry) in config.hosts.iter().enumerate() {
                let (host, route) = host_route(name, auth_scheme, index, entry, allow_plain_http)?;
                if let Some(existing) = table.hosts.get(&host) {
                    return Err(RoutingError::HostClaimedTwice {
                        host,
                        first: existing.plugin.clone(),
                        second: name.clone(),
                    });
                }
                table.hosts.insert(host, route);
            }
        }
        Ok(table)
    }

    /// Where forge calls for `repo_url` go; the forge router and the web UI both use it.
    /// `http://` URLs are only accepted by a table built with plain HTTP allowed.
    pub fn resolve(&self, repo_url: &str) -> Result<Resolved, ResolveError> {
        let url = RepoUrl::parse(repo_url).ok_or(ResolveError::InvalidUrl)?;
        if !url.is_https() && !self.allow_plain_http {
            return Err(ResolveError::NotHttps {
                host: url.authority,
            });
        }
        let route = self
            .hosts
            .get(&url.authority)
            .or(self.default.as_ref())
            .ok_or_else(|| ResolveError::Unrouted {
                host: url.authority.clone(),
            })?;
        Ok(Resolved {
            plugin: route.plugin.clone(),
            base_url: route.api_url.clone().unwrap_or(url.origin),
            auth_scheme: route.auth_scheme,
            repo: url.repo,
            host: url.authority,
        })
    }

    /// Claimed hosts with their routes, ordered by host.
    pub fn hosts(&self) -> impl Iterator<Item = (&str, &Route)> {
        self.hosts
            .iter()
            .map(|(host, route)| (host.as_str(), route))
    }

    /// The route of the plugin entry without `hosts`, if any.
    pub fn default_route(&self) -> Option<&Route> {
        self.default.as_ref()
    }

    /// Whether `http://` URLs are accepted; then plugins may also reach `http://` base URLs.
    pub fn allows_plain_http(&self) -> bool {
        self.allow_plain_http
    }
}

/// An inconsistent `plugins` configuration.
#[derive(Debug, Error)]
pub enum RoutingError {
    #[error("host `{host}` is claimed by plugins `{first}` and `{second}`")]
    HostClaimedTwice {
        host: String,
        first: String,
        second: String,
    },
    #[error("plugins {} have no `hosts`; at most one default plugin is allowed", quoted(.plugins))]
    SeveralDefaults { plugins: Vec<String> },
    #[error("plugin `{plugin}`: apiUrl `{api_url}` must use https://")]
    ApiUrlNotHttps { plugin: String, api_url: String },
    /// Not echoed: the value may carry credentials.
    #[error("plugin `{plugin}`: invalid apiUrl")]
    InvalidApiUrl { plugin: String },
    #[error("plugin `{plugin}`: invalid host in `hosts[{index}]`")]
    InvalidHost { plugin: String, index: usize },
    #[error("plugin `{plugin}`: unknown authScheme `{value}` (use `token` or `bearer`)")]
    UnknownAuthScheme { plugin: String, value: String },
}

/// Where forge calls for a repository go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Name of the plugin entry under `plugins`.
    pub plugin: String,
    /// Normalized `host[:port]` of the repository URL.
    pub host: String,
    /// The route's `apiUrl`, else the normalized origin of the repository URL.
    pub base_url: String,
    /// How the forge credential is sent.
    pub auth_scheme: AuthScheme,
    pub repo: RepoRef,
}

/// Why a repository URL has no route. Never echoes the URL: it may carry credentials.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ResolveError {
    #[error("invalid repository URL")]
    InvalidUrl,
    #[error("repository URL for host `{host}` is not HTTPS")]
    NotHttps { host: String },
    #[error("no forge plugin serves host `{host}`")]
    Unrouted { host: String },
}

/// The parts of a repository URL `scheme://host[:port]/owner/name[.git]` that routing needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoUrl {
    /// Normalized `scheme://host[:port]`, the API base URL of the default plugin.
    pub origin: String,
    /// Normalized `host[:port]`, the key of the routing table.
    pub authority: String,
    pub repo: RepoRef,
}

impl RepoUrl {
    /// `HTTPS://Git.Example.org:443/owner/name.git` → origin `https://git.example.org`,
    /// owner/name. `None` unless the scheme is `http(s)`, the path is exactly `owner/name`, and it
    /// has no userinfo, query or fragment.
    pub fn parse(repo_url: &str) -> Option<Self> {
        let url = Url::parse(repo_url).ok()?;
        if !matches!(url.scheme(), HTTPS | HTTP) || has_extras(&url) {
            return None;
        }
        let authority = authority_of(&url)?;
        let path = url.path().strip_prefix('/')?.trim_end_matches('/');
        let path = path.strip_suffix(".git").unwrap_or(path);
        let (owner, name) = path.split_once('/')?;
        if owner.is_empty() || name.is_empty() || name.contains('/') {
            return None;
        }
        Some(Self {
            origin: format!("{}://{authority}", url.scheme()),
            authority,
            repo: RepoRef {
                owner: owner.to_string(),
                name: name.to_string(),
            },
        })
    }

    /// Whether the scheme is `https`.
    pub fn is_https(&self) -> bool {
        self.origin.starts_with("https://")
    }
}

/// The routing table plus which plugins loaded: the forge of a repository URL for the web UI
/// (design D7).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForgeRoutes {
    table: RoutingTable,
    loaded: BTreeSet<String>,
}

impl ForgeRoutes {
    /// `loaded` are the names (under `plugins`) of the plugins that loaded.
    pub fn new(table: RoutingTable, loaded: impl IntoIterator<Item = String>) -> Self {
        Self {
            table,
            loaded: loaded.into_iter().collect(),
        }
    }

    /// The plugin routed for the host of `repo_url`, as the forge router would pick it.
    pub fn forge_for_repo_url(&self, repo_url: &str) -> ForgeServing {
        let Ok(Resolved { plugin, host, .. }) = self.table.resolve(repo_url) else {
            return ForgeServing::Unrouted;
        };
        if self.loaded.contains(&plugin) {
            ForgeServing::Served { plugin }
        } else {
            let default = !self.table.hosts.contains_key(&host);
            ForgeServing::NotLoaded { plugin, default }
        }
    }
}

/// Which plugin serves a repository URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForgeServing {
    /// The routed plugin loaded.
    Served { plugin: String },
    /// The routed plugin did not load; `default` unless it claims the host.
    NotLoaded { plugin: String, default: bool },
    /// Not a valid repository URL, or no plugin claims its host and there is no default.
    Unrouted,
}

impl ForgeServing {
    /// The kind of the serving plugin; `None` unless [`Self::Served`].
    pub fn kind(&self) -> Option<ForgeKind> {
        match self {
            Self::Served { plugin } => Some(ForgeKind::from_plugin(plugin)),
            Self::NotLoaded { .. } | Self::Unrouted => None,
        }
    }
}

fn known_auth_scheme(
    plugin: &str,
    setting: &AuthSchemeSetting,
) -> Result<AuthScheme, RoutingError> {
    match setting {
        AuthSchemeSetting::Known(scheme) => Ok(*scheme),
        AuthSchemeSetting::Unknown(value) => Err(RoutingError::UnknownAuthScheme {
            plugin: plugin.to_string(),
            value: value.clone(),
        }),
    }
}

fn host_route(
    plugin: &str,
    auth_scheme: AuthScheme,
    index: usize,
    entry: &HostRoute,
    allow_plain_http: bool,
) -> Result<(String, Route), RoutingError> {
    let invalid = || RoutingError::InvalidHost {
        plugin: plugin.to_string(),
        index,
    };
    let (host, api_url) = match entry {
        HostRoute::Host(raw) => {
            let host = normalize_authority(raw).ok_or_else(invalid)?;
            let api_url = format!("{HTTPS}://{host}");
            (host, api_url)
        }
        HostRoute::WithApiUrl(HostWithApiUrl { host, api_url }) => (
            normalize_authority(host).ok_or_else(invalid)?,
            normalize_api_url(plugin, api_url, allow_plain_http)?,
        ),
    };
    let route = Route {
        plugin: plugin.to_string(),
        api_url: Some(api_url),
        auth_scheme,
    };
    Ok((host, route))
}

/// `https://Host:443/api/v3/` → `https://host/api/v3`.
fn normalize_api_url(
    plugin: &str,
    api_url: &str,
    allow_plain_http: bool,
) -> Result<String, RoutingError> {
    let invalid = || RoutingError::InvalidApiUrl {
        plugin: plugin.to_string(),
    };
    let url = Url::parse(api_url).map_err(|_parse_error| invalid())?;
    if has_extras(&url) {
        return Err(invalid());
    }
    if url.scheme() != HTTPS && !(allow_plain_http && url.scheme() == HTTP) {
        return Err(RoutingError::ApiUrlNotHttps {
            plugin: plugin.to_string(),
            api_url: url.to_string(),
        });
    }
    let authority = authority_of(&url).ok_or_else(invalid)?;
    let path = url.path().trim_end_matches('/');
    Ok(format!("{}://{authority}{path}", url.scheme()))
}

/// `Name[:port]` → lower-case `name[:port]` without the HTTPS default port; `None` unless it is
/// exactly a host with an optional port.
fn normalize_authority(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let url = Url::parse(&format!("{HTTPS}://{raw}/")).ok()?;
    if url.path() != "/" || has_extras(&url) {
        return None;
    }
    authority_of(&url)
}

/// Lower-case `host[:port]` of `url`; the port only when it is not the scheme's default.
fn authority_of(url: &Url) -> Option<String> {
    let host = normalize_host(url.host_str()?);
    if host.is_empty() {
        return None;
    }
    Some(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}

/// Whether `url` has userinfo, a query or a fragment.
fn has_extras(url: &Url) -> bool {
    !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
}

fn quoted(names: &[String]) -> String {
    names
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(hosts: Vec<HostRoute>) -> PluginConfig {
        PluginConfig {
            hosts,
            ..PluginConfig::new("/plugins/p.wasm")
        }
    }

    fn host(name: &str) -> HostRoute {
        HostRoute::Host(name.to_string())
    }

    fn claim(host: &str, api_url: &str) -> HostRoute {
        HostRoute::WithApiUrl(HostWithApiUrl {
            host: host.into(),
            api_url: api_url.into(),
        })
    }

    fn table(entries: Vec<(&str, PluginConfig)>) -> Result<RoutingTable, RoutingError> {
        let plugins = entries
            .into_iter()
            .map(|(name, config)| (name.to_string(), config))
            .collect();
        RoutingTable::from_plugins(&plugins)
    }

    fn resolved(table: &RoutingTable, host: &str) -> Option<Resolved> {
        table.resolve(&format!("https://{host}/o/r")).ok()
    }

    fn plugin_for(table: &RoutingTable, host: &str) -> Option<String> {
        resolved(table, host).map(|r| r.plugin)
    }

    #[test]
    fn host_claimed_twice_names_the_host_and_both_plugins() {
        let err = table(vec![
            ("a", plugin(vec![host("git.example.org")])),
            ("b", plugin(vec![host("Git.Example.org:443")])),
        ])
        .unwrap_err();
        assert!(matches!(err, RoutingError::HostClaimedTwice { .. }));
        let message = err.to_string();
        for name in ["git.example.org", "`a`", "`b`"] {
            assert!(message.contains(name), "{message}");
        }
    }

    #[test]
    fn two_default_plugins_name_both_entries() {
        let err = table(vec![("a", plugin(vec![])), ("b", plugin(vec![]))]).unwrap_err();
        assert!(matches!(err, RoutingError::SeveralDefaults { .. }));
        let message = err.to_string();
        assert!(
            message.contains("`a`") && message.contains("`b`"),
            "{message}"
        );
    }

    #[test]
    fn port_is_part_of_the_host() {
        let table = table(vec![("a", plugin(vec![host("git.example.org:3000")]))]).unwrap();
        assert_eq!(plugin_for(&table, "git.example.org"), None);
        assert_eq!(
            plugin_for(&table, "git.example.org:3000").as_deref(),
            Some("a")
        );
    }

    #[test]
    fn host_is_lowercased_and_https_default_port_dropped() {
        let table = table(vec![("a", plugin(vec![host("Git.Example.ORG:443")]))]).unwrap();
        assert_eq!(
            table.hosts().map(|(h, _)| h).collect::<Vec<_>>(),
            ["git.example.org"]
        );
        assert_eq!(plugin_for(&table, "GIT.example.org").as_deref(), Some("a"));
        assert_eq!(
            plugin_for(&table, "git.example.org:443").as_deref(),
            Some("a")
        );
    }

    #[test]
    fn claimed_host_goes_to_its_plugin_and_others_to_the_default() {
        let table = table(vec![
            ("forgejo", plugin(vec![])),
            ("github", plugin(vec![host("github.com")])),
        ])
        .unwrap();
        assert_eq!(plugin_for(&table, "github.com").as_deref(), Some("github"));
        assert_eq!(
            plugin_for(&table, "git.example.org").as_deref(),
            Some("forgejo")
        );
        assert_eq!(table.default_route().map(|r| r.api_url.clone()), Some(None));
    }

    #[test]
    fn unclaimed_host_without_default_has_no_route() {
        let table = table(vec![("github", plugin(vec![host("github.com")]))]).unwrap();
        assert_eq!(plugin_for(&table, "gitlab.com"), None);
    }

    #[test]
    fn api_url_defaults_to_https_host() {
        let table = table(vec![("a", plugin(vec![host("GHE.example.org")]))]).unwrap();
        let route = table.hosts().next().unwrap().1;
        assert_eq!(route.api_url.as_deref(), Some("https://ghe.example.org"));
    }

    #[test]
    fn api_url_keeps_its_path_and_carries_the_auth_scheme() {
        let config = PluginConfig {
            auth_scheme: AuthScheme::Bearer.into(),
            ..plugin(vec![claim(
                "ghe.example.org",
                "HTTPS://GHE.example.org:443/api/v3/",
            )])
        };
        let table = table(vec![("github", config)]).unwrap();
        let route = resolved(&table, "ghe.example.org").unwrap();
        assert_eq!(route.base_url, "https://ghe.example.org/api/v3");
        assert_eq!(route.auth_scheme, AuthScheme::Bearer);
    }

    #[test]
    fn plain_http_api_url_is_accepted_only_by_the_test_constructor() {
        let plugins = BTreeMap::from([(
            "forgejo".to_string(),
            plugin(vec![claim(
                "127.0.0.1:3000",
                "HTTP://127.0.0.1:3000/prefix/",
            )]),
        )]);
        assert!(matches!(
            RoutingTable::from_plugins(&plugins),
            Err(RoutingError::ApiUrlNotHttps { .. })
        ));
        let table = RoutingTable::from_plugins_allowing_plain_http(&plugins).unwrap();
        assert!(table.allows_plain_http());
        let route = resolved(&table, "127.0.0.1:3000").unwrap();
        assert_eq!(route.base_url, "http://127.0.0.1:3000/prefix");
    }

    #[test]
    fn non_https_api_url_is_rejected_naming_plugin_and_value() {
        let err = table(vec![(
            "github",
            plugin(vec![claim("github.com", "http://api.github.com")]),
        )])
        .unwrap_err();
        assert!(matches!(err, RoutingError::ApiUrlNotHttps { .. }));
        let message = err.to_string();
        assert!(message.contains("`github`") && message.contains("http://api.github.com"));
    }

    #[test]
    fn api_url_with_userinfo_query_or_fragment_is_rejected_without_echoing_it() {
        for api_url in [
            "https://user:secret@api.github.com",
            "https://api.github.com?secret=1",
            "https://api.github.com#secret",
            "not a url secret",
        ] {
            let err =
                table(vec![("github", plugin(vec![claim("github.com", api_url)]))]).unwrap_err();
            assert!(
                matches!(err, RoutingError::InvalidApiUrl { .. }),
                "{api_url}"
            );
            assert!(!err.to_string().contains("secret"), "{err}");
        }
    }

    #[test]
    fn host_with_userinfo_is_rejected_without_echoing_it() {
        let err = table(vec![(
            "a",
            plugin(vec![host("user:secret@git.example.org")]),
        )])
        .unwrap_err();
        assert!(matches!(err, RoutingError::InvalidHost { .. }));
        assert!(!err.to_string().contains("secret"), "{err}");
    }

    #[test]
    fn invalid_host_is_rejected() {
        for raw in ["", "git example.org", "git.example.org:abc", "a/b", "::1"] {
            let err = table(vec![("a", plugin(vec![host(raw)]))]).unwrap_err();
            assert!(matches!(err, RoutingError::InvalidHost { .. }), "{raw}");
        }
    }

    #[test]
    fn invalid_host_names_the_plugin_and_the_entry_index_not_the_value() {
        let err = table(vec![(
            "github",
            plugin(vec![host("github.com"), claim("bad host", "https://x")]),
        )])
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "plugin `github`: invalid host in `hosts[1]`"
        );
    }

    #[test]
    fn unknown_auth_scheme_names_the_plugin_and_the_value() {
        let config: PluginConfig =
            serde_saphyr::from_str("path: /g.wasm\nhosts: [github.com]\nauthScheme: basic\n")
                .unwrap();
        let err = table(vec![("github", config)]).unwrap_err();
        assert!(matches!(err, RoutingError::UnknownAuthScheme { .. }));
        assert_eq!(
            err.to_string(),
            "plugin `github`: unknown authScheme `basic` (use `token` or `bearer`)"
        );
    }

    #[test]
    fn bracketed_ipv6_host_keeps_its_port() {
        let table = table(vec![("a", plugin(vec![host("[::1]:3000")]))]).unwrap();
        assert_eq!(plugin_for(&table, "[::1]:3000").as_deref(), Some("a"));
        assert_eq!(plugin_for(&table, "[::1]"), None);
    }

    #[test]
    fn repo_url_splits_origin_authority_owner_and_name() {
        let url = RepoUrl::parse("https://git.example.org/peter/dark-factory.git").unwrap();
        assert_eq!(url.origin, "https://git.example.org");
        assert_eq!(url.authority, "git.example.org");
        assert_eq!(
            (url.repo.owner.as_str(), url.repo.name.as_str()),
            ("peter", "dark-factory")
        );
        assert!(url.is_https());
        assert!(RepoUrl::parse("https://git.example.org:3000/o/r/").is_some());
        assert!(
            !RepoUrl::parse("http://git.example.org/o/r")
                .unwrap()
                .is_https()
        );
        assert!(RepoUrl::parse("git@git.example.org:o/r.git").is_none());
        assert!(RepoUrl::parse("https://git.example.org/only-owner").is_none());
        assert!(RepoUrl::parse("https://git.example.org/a/b/c").is_none());
        assert!(RepoUrl::parse("ssh://git.example.org/o/r").is_none());
    }

    #[test]
    fn repo_url_origin_and_authority_are_normalized() {
        for (raw, origin, authority) in [
            (
                "HTTPS://Git.Example.ORG:443/o/r",
                "https://git.example.org",
                "git.example.org",
            ),
            (
                "https://git.example.org.:3000/o/r",
                "https://git.example.org:3000",
                "git.example.org:3000",
            ),
            ("http://h.example:80/o/r", "http://h.example", "h.example"),
            (
                "http://h.example:443/o/r",
                "http://h.example:443",
                "h.example:443",
            ),
            (
                "https://[0:0::1]:3000/o/r",
                "https://[::1]:3000",
                "[::1]:3000",
            ),
        ] {
            let url = RepoUrl::parse(raw).unwrap();
            assert_eq!(
                (url.origin.as_str(), url.authority.as_str()),
                (origin, authority)
            );
        }
    }

    #[test]
    fn repo_url_with_userinfo_query_or_fragment_is_rejected() {
        for raw in [
            "https://user:token@github.com/o/r",
            "https://token@github.com/o/r",
            "https://github.com/o/r?token=1",
            "https://github.com/o/r#token",
        ] {
            assert!(RepoUrl::parse(raw).is_none(), "{raw}");
        }
    }

    #[test]
    fn resolve_rejects_userinfo_without_echoing_it() {
        let table = table(vec![("github", plugin(vec![host("github.com")]))]).unwrap();
        let err = table
            .resolve("https://user:token@github.com/o/r")
            .unwrap_err();
        assert_eq!(err, ResolveError::InvalidUrl);
        let message = err.to_string();
        assert!(
            !message.contains("user") && !message.contains("token"),
            "{message}"
        );
    }

    #[test]
    fn resolve_uses_the_claimed_api_url_or_the_normalized_origin() {
        let config = PluginConfig {
            auth_scheme: AuthScheme::Bearer.into(),
            ..plugin(vec![claim("github.com", "https://api.github.com")])
        };
        let table = table(vec![("github", config), ("forgejo", plugin(vec![]))]).unwrap();
        let repo = |owner: &str, name: &str| RepoRef {
            owner: owner.into(),
            name: name.into(),
        };
        assert_eq!(
            table.resolve("https://GitHub.com/acme/web.git"),
            Ok(Resolved {
                plugin: "github".into(),
                host: "github.com".into(),
                base_url: "https://api.github.com".into(),
                auth_scheme: AuthScheme::Bearer,
                repo: repo("acme", "web"),
            })
        );
        assert_eq!(
            table.resolve("HTTPS://Git.Example.org:443/peter/df/"),
            Ok(Resolved {
                plugin: "forgejo".into(),
                host: "git.example.org".into(),
                base_url: "https://git.example.org".into(),
                auth_scheme: AuthScheme::Token,
                repo: repo("peter", "df"),
            })
        );
    }

    #[test]
    fn resolve_names_the_normalized_host_of_an_unrouted_url() {
        let table = table(vec![("github", plugin(vec![host("github.com")]))]).unwrap();
        assert_eq!(
            table.resolve("https://GitLab.com:8443/a/b"),
            Err(ResolveError::Unrouted {
                host: "gitlab.com:8443".into()
            })
        );
    }

    #[test]
    fn plain_http_repo_url_resolves_only_when_the_table_allows_it() {
        let plugins = BTreeMap::from([("forgejo".to_string(), plugin(vec![]))]);
        let url = "http://127.0.0.1:3000/o/r.git";
        let strict = RoutingTable::from_plugins(&plugins).unwrap();
        assert_eq!(
            strict.resolve(url),
            Err(ResolveError::NotHttps {
                host: "127.0.0.1:3000".into()
            })
        );
        let lenient = RoutingTable::from_plugins_allowing_plain_http(&plugins).unwrap();
        assert_eq!(
            lenient.resolve(url).map(|r| r.base_url),
            Ok("http://127.0.0.1:3000".to_string())
        );
    }

    #[test]
    fn plain_http_repo_url_in_production_names_the_host_without_userinfo() {
        let table = table(vec![("forgejo", plugin(vec![]))]).unwrap();
        let err = table.resolve("HTTP://Git.Example.org/o/r").unwrap_err();
        assert_eq!(
            err.to_string(),
            "repository URL for host `git.example.org` is not HTTPS"
        );
        let err = table
            .resolve("http://user:token@git.example.org/o/r")
            .unwrap_err();
        assert!(!err.to_string().contains("token"), "{err}");
        assert_eq!(
            ForgeRoutes::new(table, ["forgejo".to_string()])
                .forge_for_repo_url("http://git.example.org/o/r"),
            ForgeServing::Unrouted
        );
    }

    fn forge_routes(loaded: &[&str]) -> ForgeRoutes {
        let table = table(vec![
            ("forgejo", plugin(vec![])),
            ("github", plugin(vec![host("github.com")])),
        ])
        .unwrap();
        ForgeRoutes::new(table, loaded.iter().map(ToString::to_string))
    }

    fn served(plugin: &str) -> ForgeServing {
        ForgeServing::Served {
            plugin: plugin.to_string(),
        }
    }

    #[test]
    fn claimed_host_is_served_by_its_loaded_plugin() {
        let serving = forge_routes(&["forgejo", "github"])
            .forge_for_repo_url("https://github.com/acme/web.git");
        assert_eq!(serving, served("github"));
        assert_eq!(serving.kind(), Some(ForgeKind::GitHub));
    }

    #[test]
    fn unclaimed_host_is_served_by_the_loaded_default() {
        let serving = forge_routes(&["forgejo", "github"])
            .forge_for_repo_url("https://git.example.org/peter/dark-factory.git");
        assert_eq!(serving, served("forgejo"));
        assert_eq!(serving.kind(), Some(ForgeKind::Forgejo));
    }

    #[test]
    fn claiming_plugin_that_did_not_load_is_named_not_loaded() {
        let serving = forge_routes(&["forgejo"]).forge_for_repo_url("https://github.com/acme/web");
        assert_eq!(
            serving,
            ForgeServing::NotLoaded {
                plugin: "github".into(),
                default: false,
            }
        );
        assert_eq!(serving.kind(), None);
    }

    #[test]
    fn default_plugin_that_did_not_load_is_named_not_loaded_default() {
        let serving =
            forge_routes(&["github"]).forge_for_repo_url("https://git.example.org/acme/web");
        assert_eq!(
            serving,
            ForgeServing::NotLoaded {
                plugin: "forgejo".into(),
                default: true,
            }
        );
        assert_eq!(serving.kind(), None);
    }

    #[test]
    fn unclaimed_host_without_default_is_unrouted() {
        let table = table(vec![("github", plugin(vec![host("github.com")]))]).unwrap();
        let routes = ForgeRoutes::new(table, ["github".to_string()]);
        let serving = routes.forge_for_repo_url("https://gitlab.com/acme/web.git");
        assert_eq!(serving, ForgeServing::Unrouted);
        assert_eq!(serving.kind(), None);
    }

    #[test]
    fn url_that_is_not_an_https_repository_is_unrouted() {
        let routes = forge_routes(&["forgejo", "github"]);
        for url in [
            "",
            "git@github.com:acme/web.git",
            "http://github.com/acme/web.git",
            "https://github.com/only-owner",
            "https://github.com/a/b/c",
            "https://git hub.com/acme/web",
            "https://user:token@github.com/acme/web",
            "https://github.com/acme/web?x=1",
        ] {
            assert_eq!(
                routes.forge_for_repo_url(url),
                ForgeServing::Unrouted,
                "{url}"
            );
        }
    }

    #[test]
    fn host_case_and_default_port_reach_the_same_route() {
        let routes = forge_routes(&["forgejo", "github"]);
        for url in [
            "https://GitHub.com/acme/web.git",
            "HTTPS://github.com:443/acme/web",
        ] {
            assert_eq!(routes.forge_for_repo_url(url), served("github"), "{url}");
        }
        assert_eq!(
            routes.forge_for_repo_url("https://github.com:8443/acme/web"),
            served("forgejo")
        );
    }
}
