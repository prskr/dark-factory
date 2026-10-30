//! Plugin settings from `wasi:config/store`: API endpoints, API version and draft handling.

use crate::darkfactory::integrations::types::Error;
use crate::draft;

/// `X-GitHub-Api-Version` unless `api-version` is set: the current version on github.com.
const DEFAULT_API_VERSION: &str = "2026-03-10";

/// How new drafts are marked; reading recognizes both forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftMode {
    /// The first work-in-progress prefix in the title, for repositories or instances that reject
    /// native drafts.
    TitlePrefix,
    /// GitHub's draft pull requests, the default.
    Native,
    /// `native`, falling back to `title-prefix` with a warning where GitHub has no drafts.
    Auto,
}

impl DraftMode {
    /// Whether `create` asks GitHub for a native draft.
    pub(crate) fn creates_native_drafts(self) -> bool {
        match self {
            Self::Native | Self::Auto => true,
            Self::TitlePrefix => false,
        }
    }

    /// Whether `create` retries with the title prefix where GitHub has no drafts.
    pub(crate) fn falls_back_to_prefix(self) -> bool {
        match self {
            Self::Auto => true,
            Self::TitlePrefix | Self::Native => false,
        }
    }

    /// Whether a ready pull request becomes a draft through GitHub's mutation, not the prefix.
    pub(crate) fn converts_natively(self) -> bool {
        match self {
            Self::Native => true,
            Self::TitlePrefix | Self::Auto => false,
        }
    }

    /// Whether the mode writes the title prefix, so `wip-prefixes` must not be empty.
    pub(crate) fn needs_prefixes(self) -> bool {
        match self {
            Self::TitlePrefix | Self::Auto => true,
            Self::Native => false,
        }
    }
}

/// Validated settings of one plugin instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `base-url` without a trailing slash; REST paths are appended to it.
    pub(crate) rest_url: String,
    pub(crate) graphql_url: String,
    pub(crate) api_version: String,
    pub(crate) draft_mode: DraftMode,
    /// Recognized work-in-progress prefixes; the first one marks new drafts.
    pub(crate) prefixes: Vec<String>,
}

impl Settings {
    /// Reads `base-url`, `api-version`, `draft-mode` and `wip-prefixes` through `get`.
    pub fn read(get: impl Fn(&str) -> Result<Option<String>, Error>) -> Result<Self, Error> {
        let base_url = get("base-url")?
            .ok_or_else(|| Error::Config("`base-url` is not configured".to_string()))?;
        let (rest_url, graphql_url) = endpoints(&base_url)?;
        let draft_mode = match get("draft-mode")?.as_deref() {
            None | Some("native") => DraftMode::Native,
            Some("title-prefix") => DraftMode::TitlePrefix,
            Some("auto") => DraftMode::Auto,
            Some(other) => {
                return Err(Error::Config(format!(
                    "unknown `draft-mode` `{other}`: use `native`, `title-prefix` or `auto`"
                )));
            }
        };
        let prefixes = get("wip-prefixes")?;
        let prefixes =
            draft::parse_prefixes(prefixes.as_deref().unwrap_or(draft::DEFAULT_PREFIXES));
        if prefixes.is_empty() && draft_mode.needs_prefixes() {
            return Err(Error::Config(
                "`wip-prefixes` is empty: `title-prefix` and `auto` mode need at least one prefix"
                    .to_string(),
            ));
        }
        Ok(Self {
            rest_url,
            graphql_url,
            api_version: get("api-version")?.unwrap_or_else(|| DEFAULT_API_VERSION.to_string()),
            draft_mode,
            prefixes,
        })
    }
}

/// REST base URL and GraphQL endpoint of `base_url`.
fn endpoints(base_url: &str) -> Result<(String, String), Error> {
    let rest = base_url.trim_end_matches('/');
    let (scheme, target) = rest
        .split_once("://")
        .ok_or_else(|| Error::Config(format!("`base-url` has no scheme: {base_url}")))?;
    let (authority, path) = target.split_once('/').unwrap_or((target, ""));
    if authority.is_empty() {
        return Err(Error::Config(format!("`base-url` has no host: {base_url}")));
    }
    let host = authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host);
    if host.eq_ignore_ascii_case("github.com") {
        return Err(Error::Config(format!(
            "`base-url` {base_url} is GitHub's web host, not its API: set `apiUrl: https://api.github.com` on the route"
        )));
    }
    // GitHub Enterprise Server serves REST below `/api/v3` and GraphQL at `/api/graphql`.
    let graphql = if path == "api/v3" || path.ends_with("/api/v3") {
        format!("{scheme}://{authority}/api/graphql")
    } else {
        format!("{rest}/graphql")
    };
    Ok((rest.to_string(), graphql))
}
