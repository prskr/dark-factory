//! What a host is to the projects of this instance: the rule that maps a project's repository and
//! issue tracker URLs to an icon, shared by the project pages and the egress pages.

use std::collections::HashMap;

use df_core::ForgeKind;
use df_core::host::{normalize_host, strip_brackets};
use df_core::routing::ForgeRoutes;
use df_db::catalog::ProjectRecord;
use maud::{Markup, html};
use url::Url;

use super::icons::{Icon, forge_icon, icon};

/// What a host is to the projects of this instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostRole {
    /// The repository host; `None` when no forge integration serves the project.
    Forge(Option<ForgeKind>),
    /// An issue tracker on a host other than the repository's.
    IssueTracker,
}

impl HostRole {
    /// The `data-host-icon` value, for tests.
    pub(crate) fn data(&self) -> &'static str {
        match self {
            Self::Forge(Some(ForgeKind::Forgejo)) => "forge-forgejo",
            Self::Forge(Some(ForgeKind::GitHub)) => "forge-github",
            Self::Forge(_) => "forge-git",
            Self::IssueTracker => "issue",
        }
    }

    /// Which role wins when two projects claim a host.
    fn strength(&self) -> u8 {
        match self {
            Self::Forge(Some(_)) => 2,
            Self::Forge(None) => 1,
            Self::IssueTracker => 0,
        }
    }
}

/// The icon of a role.
pub fn role_icon(role: &HostRole) -> Markup {
    match role {
        HostRole::Forge(kind) => forge_icon(kind.as_ref()),
        HostRole::IssueTracker => html! {
            span class="inline-flex shrink-0" role="img" aria-label="Issue tracker" title="Issue tracker" data-forge="issue" { (icon(Icon::Issue)) }
        },
    }
}

/// The role of the issue tracker host: the forge's when it is the repository host and a forge
/// serves the project (`kind`), the issue tracker's otherwise.
pub fn issue_role(
    kind: Option<&ForgeKind>,
    repo_url: &str,
    issue_tracker_url: &str,
) -> Option<(String, HostRole)> {
    let host = url_host(issue_tracker_url)?;
    let role = if kind.is_some() && url_host(repo_url).as_deref() == Some(host.as_str()) {
        HostRole::Forge(kind.cloned())
    } else {
        HostRole::IssueTracker
    };
    Some((host, role))
}

/// The integration that serves `p`: the loaded plugin routed for its repository URL when the
/// project has a forge credential, none (generic Git) otherwise.
pub fn project_forge_kind(forges: &ForgeRoutes, p: &ProjectRecord) -> Option<ForgeKind> {
    p.forge_credential_id
        .and_then(|_| forges.forge_for_repo_url(&p.repo_url).kind())
}

/// The hosts of all projects by normalized host.
#[derive(Debug, Default, Clone)]
pub struct ProjectHosts(HashMap<String, HostRole>);

impl ProjectHosts {
    /// `forges` routes the projects with a forge credential to their integration.
    pub fn from_projects(forges: &ForgeRoutes, projects: &[ProjectRecord]) -> Self {
        let mut hosts: HashMap<String, HostRole> = HashMap::new();
        for p in projects {
            let kind = project_forge_kind(forges, p);
            for (host, role) in
                project_roles(kind.as_ref(), &p.repo_url, p.issue_tracker_url.as_deref())
            {
                match hosts.get(&host) {
                    Some(known) if known.strength() >= role.strength() => {}
                    _ => {
                        hosts.insert(host, role);
                    }
                }
            }
        }
        Self(hosts)
    }

    /// The role of `host`; exact match only, so a wildcard entry or a subdomain finds nothing.
    pub fn get(&self, host: &str) -> Option<&HostRole> {
        let host = normalize_host(host.trim());
        self.0.get(strip_brackets(&host))
    }
}

/// The hosts of one project with their roles. `kind` is the integration that serves the project.
fn project_roles(
    kind: Option<&ForgeKind>,
    repo_url: &str,
    issue_tracker_url: Option<&str>,
) -> Vec<(String, HostRole)> {
    let repo = url_host(repo_url).map(|host| (host, HostRole::Forge(kind.cloned())));
    let issue = issue_tracker_url.and_then(|url| issue_role(kind, repo_url, url));
    repo.into_iter().chain(issue).collect()
}

/// Lenient host extraction of a project URL (the form uses the stricter `ForgeRoutes`), in
/// the normalized form the lookup compares.
fn url_host(url: &str) -> Option<String> {
    let url = Url::parse(url.trim()).ok()?;
    let host = normalize_host(url.host_str()?);
    Some(strip_brackets(&host).to_string())
}

/// A project for tests of this module and of `host_icon`.
#[cfg(test)]
pub(crate) fn test_project(repo: &str, issue: Option<&str>, credential: bool) -> ProjectRecord {
    ProjectRecord {
        id: uuid::Uuid::nil(),
        key: "DF".into(),
        name: "DF".into(),
        repo_url: repo.into(),
        issue_tracker_url: issue.map(Into::into),
        image: String::new(),
        base_branch: "main".into(),
        adapter_command: Vec::new(),
        forge_credential_id: credential.then(uuid::Uuid::nil),
        has_tasks: false,
    }
}

#[cfg(test)]
mod tests {
    use super::test_project as project;
    use super::*;
    use crate::testkit::default_forge;

    const REPO: &str = "https://git.example.internal/org/df.git";

    fn roles(kind: Option<&ForgeKind>, issue: Option<&str>) -> Vec<(String, HostRole)> {
        project_roles(kind, REPO, issue)
    }

    #[test]
    fn the_repository_host_is_the_forge() {
        let forgejo = ForgeKind::Forgejo;
        assert_eq!(
            roles(Some(&forgejo), None),
            [(
                "git.example.internal".into(),
                HostRole::Forge(Some(ForgeKind::Forgejo))
            )]
        );
        assert_eq!(
            roles(None, None),
            [("git.example.internal".into(), HostRole::Forge(None))]
        );
    }

    #[test]
    fn a_tracker_on_the_repository_host_follows_the_forge_only_when_one_serves_it() {
        let issue = Some("https://git.example.internal/org/df/issues");
        let served = roles(Some(&ForgeKind::Forgejo), issue);
        assert_eq!(served[1].1, HostRole::Forge(Some(ForgeKind::Forgejo)));
        assert_eq!(roles(None, issue)[1].1, HostRole::IssueTracker);
    }

    #[test]
    fn a_tracker_on_the_repository_host_with_a_trailing_dot_follows_the_forge() {
        let found = issue_role(
            Some(&ForgeKind::Forgejo),
            REPO,
            "https://git.example.internal./org/df/issues",
        );
        assert_eq!(
            found,
            Some((
                "git.example.internal".into(),
                HostRole::Forge(Some(ForgeKind::Forgejo))
            ))
        );
    }

    #[test]
    fn a_tracker_on_another_host_is_an_issue_tracker() {
        let found = roles(
            Some(&ForgeKind::Forgejo),
            Some("https://jira.example.com/DF"),
        );
        assert_eq!(
            found[1],
            ("jira.example.com".into(), HostRole::IssueTracker)
        );
    }

    #[test]
    fn unparsable_urls_have_no_entry() {
        assert!(project_roles(None, "not a url", Some("also not")).is_empty());
    }

    #[test]
    fn lookup_ignores_case_a_trailing_dot_and_brackets() {
        let hosts = ProjectHosts::from_projects(
            &default_forge("forgejo"),
            &[
                project(REPO, None, true),
                project("https://[2001:db8::1]/o/r.git", None, true),
            ],
        );
        for host in [
            "Git.Example.Internal",
            "git.example.internal.",
            " git.example.internal",
        ] {
            assert!(hosts.get(host).is_some(), "{host}");
        }
        assert!(hosts.get("2001:db8::1").is_some());
        assert!(hosts.get("[2001:db8::1]").is_some());
    }

    #[test]
    fn the_stronger_role_wins_in_both_orders() {
        let served = project(REPO, None, true);
        let unserved = project(REPO, None, false);
        let tracker = project("https://other.example/o/r.git", Some(REPO), true);
        let forge = &default_forge("forgejo");
        for projects in [[&served, &unserved], [&unserved, &served]] {
            let hosts = ProjectHosts::from_projects(forge, &projects.map(Clone::clone));
            assert_eq!(
                hosts.get("git.example.internal"),
                Some(&HostRole::Forge(Some(ForgeKind::Forgejo)))
            );
        }
        for projects in [[&unserved, &tracker], [&tracker, &unserved]] {
            let hosts = ProjectHosts::from_projects(forge, &projects.map(Clone::clone));
            assert_eq!(
                hosts.get("git.example.internal"),
                Some(&HostRole::Forge(None))
            );
        }
    }

    #[test]
    fn subdomains_and_wildcards_do_not_match() {
        let hosts =
            ProjectHosts::from_projects(&ForgeRoutes::default(), &[project(REPO, None, false)]);
        assert!(hosts.get("api.git.example.internal").is_none());
        assert!(hosts.get("*.git.example.internal").is_none());
        assert!(hosts.get("example.internal").is_none());
    }
}
