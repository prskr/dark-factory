//! The icon of an external host: the forge or issue-tracker icon of a project host, a brand
//! monogram for well-known hosts, the globe otherwise. Brands draw only letters.

use df_core::host::{is_ip_literal, normalize_host};
use maud::{Markup, html};

use super::icons::{Icon, icon};
use super::project_host::{ProjectHosts, role_icon};

/// Where the icon is used; sets its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostIconSize {
    /// Rows of the egress tables.
    Table,
    /// Destination nodes of the connection graph.
    Graph,
    /// Header of the request details panel.
    Panel,
}

/// Provider domain, monogram and the tone classes (theme aware status tokens). A host belongs to
/// a provider if it is that domain or a subdomain of it.
const BRANDS: &[(&str, &str, &str)] = &[
    (
        "anthropic.com",
        "A",
        "bg-status-limited-bg text-status-limited-fg",
    ),
    (
        "github.com",
        "GH",
        "bg-status-queued-bg text-status-queued-fg",
    ),
    (
        "githubusercontent.com",
        "GH",
        "bg-status-queued-bg text-status-queued-fg",
    ),
    (
        "npmjs.org",
        "n",
        "bg-status-failed-bg text-status-failed-fg",
    ),
    (
        "golang.org",
        "Go",
        "bg-status-running-bg text-status-running-fg",
    ),
    (
        "pypi.org",
        "Py",
        "bg-status-waiting-bg text-status-waiting-fg",
    ),
    (
        "pythonhosted.org",
        "Py",
        "bg-status-waiting-bg text-status-waiting-fg",
    ),
    (
        "googleapis.com",
        "G",
        "bg-status-completed-bg text-status-completed-fg",
    ),
];

/// The icon of `host`, decorative: the host name is always written next to it. Precedence: a
/// project host, a provider monogram, an IP literal, the globe.
pub fn host_icon(host: &str, size: HostIconSize, projects: &ProjectHosts) -> Markup {
    let (frame, text) = match size {
        HostIconSize::Table => ("size-6 rounded-md", "text-[10px]"),
        HostIconSize::Graph => ("size-7 rounded-md", "text-[11px]"),
        HostIconSize::Panel => ("size-9 rounded-lg", "text-sm"),
    };
    html! {
        @if let Some(role) = projects.get(host) {
            span aria-hidden="true" data-host-icon=(role.data())
                class={"inline-flex shrink-0 items-center justify-center bg-sunk text-muted [&>svg]:size-4 " (frame)} {
                (role_icon(role))
            }
        } @else if let Some((monogram, tone)) = brand(host) {
            span aria-hidden="true" data-host-icon=(monogram)
                class={"inline-flex shrink-0 items-center justify-center font-semibold " (frame) " " (text) " " (tone)} {
                (monogram)
            }
        } @else {
            @let (kind, glyph) = if is_ip_literal(host) { ("server", Icon::Server) } else { ("globe", Icon::Globe) };
            span aria-hidden="true" data-host-icon=(kind)
                class={"inline-flex shrink-0 items-center justify-center bg-sunk text-muted [&>svg]:size-4 " (frame)} {
                (icon(glyph))
            }
        }
    }
}

/// Monogram and tone classes of `host`; a wildcard entry (`*.github.com`) counts as its suffix.
fn brand(host: &str) -> Option<(&'static str, &'static str)> {
    let host = normalize_host(host.trim());
    let host = host.strip_prefix("*.").unwrap_or(&host);
    BRANDS
        .iter()
        .find(|(domain, ..)| {
            host == *domain
                || host
                    .strip_suffix(domain)
                    .is_some_and(|rest| rest.ends_with('.'))
        })
        .map(|(_, monogram, tone)| (*monogram, *tone))
}

#[cfg(test)]
mod tests {
    use super::super::project_host::test_project as project;
    use super::*;

    fn rendered(host: &str) -> String {
        host_icon(host, HostIconSize::Table, &ProjectHosts::default()).into_string()
    }

    fn with_projects(
        plugin: &str,
        projects: &[df_db::catalog::ProjectRecord],
        host: &str,
    ) -> String {
        let hosts = ProjectHosts::from_projects(&crate::testkit::default_forge(plugin), projects);
        host_icon(host, HostIconSize::Table, &hosts).into_string()
    }

    #[test]
    fn project_hosts_show_the_forge_icons_and_the_issue_icon() {
        let forgejo = [project(
            "https://git.example.internal/o/r.git",
            Some("https://jira.example.com/DF"),
            true,
        )];
        assert!(
            with_projects("forgejo", &forgejo, "git.example.internal")
                .contains("data-host-icon=\"forge-forgejo\"")
        );
        assert!(
            with_projects("forgejo", &forgejo, "jira.example.com")
                .contains("data-host-icon=\"issue\"")
        );
        let github = [project("https://code.example.org/o/r.git", None, true)];
        assert!(
            with_projects("github", &github, "code.example.org")
                .contains("data-host-icon=\"forge-github\"")
        );
        let plain = [project("https://code.example.org/o/r.git", None, false)];
        assert!(
            with_projects("github", &plain, "code.example.org")
                .contains("data-host-icon=\"forge-git\"")
        );
    }

    #[test]
    fn a_project_host_beats_the_monogram_but_only_for_that_exact_host() {
        let projects = [project("https://github.com/o/r.git", None, true)];
        let github = with_projects("github", &projects, "github.com");
        assert!(github.contains("data-host-icon=\"forge-github\"") && !github.contains(">GH<"));
        let api = with_projects("github", &projects, "api.github.com");
        assert!(api.contains("data-host-icon=\"GH\""));
    }

    #[test]
    fn known_hosts_show_their_monogram() {
        let html = rendered("github.com");
        assert!(
            html.contains("data-host-icon=\"GH\"") && html.contains(">GH<"),
            "{html}"
        );
        assert!(html.contains("aria-hidden=\"true\""));
    }

    #[test]
    fn a_wildcard_entry_uses_its_suffix() {
        assert!(rendered("*.github.com").contains("data-host-icon=\"GH\""));
        assert!(rendered("*.pypi.org").contains("data-host-icon=\"Py\""));
    }

    #[test]
    fn subdomains_of_a_provider_match_on_a_label_boundary() {
        for host in [
            "api.github.com",
            "codeload.github.com",
            "github.com",
            "API.GitHub.com",
            "github.com.",
        ] {
            assert!(rendered(host).contains("data-host-icon=\"GH\""), "{host}");
        }
        assert!(rendered("raw.githubusercontent.com").contains("data-host-icon=\"GH\""));
        assert!(rendered("files.pythonhosted.org").contains("data-host-icon=\"Py\""));
        for host in ["evilgithub.com", "github.com.evil.example", "xpypi.org"] {
            assert!(
                rendered(host).contains("data-host-icon=\"globe\""),
                "{host}"
            );
        }
    }

    #[test]
    fn ip_literals_get_the_server_icon_and_other_names_the_globe() {
        for host in [
            "10.0.0.1",
            "203.0.113.42",
            "::1",
            "[::1]",
            "2606:4700::1111",
        ] {
            let html = rendered(host);
            assert!(
                html.contains("data-host-icon=\"server\"") && html.contains("<svg"),
                "{host}"
            );
        }
        for host in ["ghcr.io", "", "localhost", "[[::1", "::1]", "[::1"] {
            assert!(
                rendered(host).contains("data-host-icon=\"globe\""),
                "{host}"
            );
        }
    }

    #[test]
    fn sizes_differ() {
        let table =
            host_icon("pypi.org", HostIconSize::Table, &ProjectHosts::default()).into_string();
        let graph =
            host_icon("pypi.org", HostIconSize::Graph, &ProjectHosts::default()).into_string();
        let panel =
            host_icon("pypi.org", HostIconSize::Panel, &ProjectHosts::default()).into_string();
        assert!(table.contains("size-6") && graph.contains("size-7") && panel.contains("size-9"));
    }
}
