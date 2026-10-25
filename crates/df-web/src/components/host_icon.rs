//! The icon of an external host: a brand monogram for well-known hosts, the globe otherwise.
//! Only letters are drawn, no logos are embedded.

use df_core::host::{is_ip_literal, normalize_host};
use maud::{Markup, html};

use super::icons::{Icon, icon};

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

/// The icon of `host`, decorative: the host name is always written next to it.
pub fn host_icon(host: &str, size: HostIconSize) -> Markup {
    let (frame, text) = match size {
        HostIconSize::Table => ("size-6 rounded-md", "text-[10px]"),
        HostIconSize::Graph => ("size-7 rounded-md", "text-[11px]"),
        HostIconSize::Panel => ("size-9 rounded-lg", "text-sm"),
    };
    html! {
        @if let Some((monogram, tone)) = brand(host) {
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
    use super::*;

    fn rendered(host: &str) -> String {
        host_icon(host, HostIconSize::Table).into_string()
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
        let table = host_icon("pypi.org", HostIconSize::Table).into_string();
        let graph = host_icon("pypi.org", HostIconSize::Graph).into_string();
        let panel = host_icon("pypi.org", HostIconSize::Panel).into_string();
        assert!(table.contains("size-6") && graph.contains("size-7") && panel.contains("size-9"));
    }
}
