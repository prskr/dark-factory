//! The icon of an external host: the forge or issue-tracker icon of a project host, the glyph
//! of a well-known provider (letter monogram if it has none), the globe otherwise.

use std::sync::LazyLock;

use df_core::host::{is_ip_literal, normalize_host};
use maud::{Markup, html};

use super::badges::Tone;
use super::icons::{Icon, icon};
use super::project_host::{ProjectHosts, role_icon};

/// The providers, embedded from `brands.json`; add one by adding an object there. A host belongs
/// to a provider if it is one of its domains or a subdomain of one. `path` is the `d` attribute
/// of the 24x24 Simple Icons glyph (CC0 1.0, <https://simpleicons.org>, simple-icons@14.0.0),
/// shown only to identify the destination.
static BRANDS: LazyLock<Vec<Brand>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("brands.json"))
        .expect("brands.json is compiled in and checked by tests")
});

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

/// The icon of `host`, decorative: the host name is always written next to it. Precedence: a
/// project host, a provider glyph or monogram, an IP literal, the globe.
pub fn host_icon(host: &str, size: HostIconSize, projects: &ProjectHosts) -> Markup {
    let (frame, text) = match size {
        HostIconSize::Table => ("size-6 rounded-md", "text-[10px]"),
        HostIconSize::Graph => ("size-7 rounded-md", "text-[11px]"),
        HostIconSize::Panel => ("size-9 rounded-lg", "text-sm"),
    };
    html! {
        @if let Some(role) = projects.get(host) {
            span aria-hidden="true" data-host-icon=(role.data())
                class={"inline-flex shrink-0 items-center justify-center bg-sunk text-muted [&_svg]:size-4 " (frame)} {
                (role_icon(role))
            }
        } @else if let Some(brand) = brand_in(&BRANDS, host) {
            (brand_tile(brand, frame, text))
        } @else {
            @let (kind, glyph) = if is_ip_literal(host) { ("server", Icon::Server) } else { ("globe", Icon::Globe) };
            span aria-hidden="true" data-host-icon=(kind)
                class={"inline-flex shrink-0 items-center justify-center bg-sunk text-muted [&_svg]:size-4 " (frame)} {
                (icon(glyph))
            }
        }
    }
}

/// The glyph of the provider `name` as inline svg, sized and coloured like [`icon`]. A provider
/// without a path shows the generic Git icon, never an empty one; tests pin the `github` entry.
pub(super) fn brand_glyph(name: &str) -> Markup {
    BRANDS
        .iter()
        .find(|brand| brand.name == name)
        .and_then(|brand| brand.path.as_deref())
        .map_or_else(|| icon(Icon::Git), glyph_svg)
}

/// A known provider: `name` for `data-host-icon="brand-<name>"`, the letter monogram of the
/// fallback, the tone and the glyph's path data, if there is one.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Brand {
    name: String,
    domains: Vec<String>,
    monogram: String,
    tone: Tone,
    path: Option<String>,
}

fn glyph_svg(path: &str) -> Markup {
    html! {
        svg class="size-5 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor"
            stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" {
            path fill="currentColor" stroke="none" d=(path) {}
        }
    }
}

/// The tile of a provider: its glyph, or the monogram without one.
fn brand_tile(brand: &Brand, frame: &str, text: &str) -> Markup {
    html! {
        @if let Some(path) = &brand.path {
            span aria-hidden="true" data-host-icon={"brand-" (brand.name)}
                class={"inline-flex shrink-0 items-center justify-center [&_svg]:size-4 " (frame) " " (brand.tone.class())} {
                (glyph_svg(path))
            }
        } @else {
            span aria-hidden="true" data-host-icon=(brand.monogram)
                class={"inline-flex shrink-0 items-center justify-center font-semibold " (frame) " " (text) " " (brand.tone.class())} {
                (brand.monogram)
            }
        }
    }
}

/// The provider of `host` in `brands`; a wildcard entry (`*.github.com`) counts as its suffix.
fn brand_in<'a>(brands: &'a [Brand], host: &str) -> Option<&'a Brand> {
    let host = normalize_host(host.trim());
    let host = host.strip_prefix("*.").unwrap_or(&host);
    brands.iter().find(|brand| {
        brand.domains.iter().any(|domain| {
            host == domain
                || host
                    .strip_suffix(domain.as_str())
                    .is_some_and(|rest| rest.ends_with('.'))
        })
    })
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
        let api = with_projects("github", &projects, "evil.github.com");
        assert!(api.contains("data-host-icon=\"brand-github\""));
    }

    #[test]
    fn known_hosts_show_their_glyph() {
        let github = rendered("api.github.com");
        assert!(
            github.contains("data-host-icon=\"brand-github\"")
                && github.contains("M12 .297c-6.63")
                && !github.contains(">GH<"),
            "{github}"
        );
        assert!(github.contains("aria-hidden=\"true\""));
        let anthropic = rendered("api.anthropic.com");
        assert!(
            anthropic.contains("data-host-icon=\"brand-anthropic\"")
                && anthropic.contains("M17.3041 3.541"),
            "{anthropic}"
        );
    }

    #[test]
    fn every_known_provider_domain_shows_its_glyph() {
        for (host, name, prefix) in [
            ("github.com", "github", "M12 .297c-6.63 0-1"),
            ("githubusercontent.com", "github", "M12 .297c-6.63 0-1"),
            ("ghcr.io", "github", "M12 .297c-6.63 0-1"),
            ("github.io", "github", "M12 .297c-6.63 0-1"),
            ("prskr.github.io", "github", "M12 .297c-6.63 0-1"),
            ("anthropic.com", "anthropic", "M17.3041 3.541h-3."),
            ("npmjs.org", "npm", "M1.763 0C.786 0 0 "),
            ("golang.org", "go", "M1.811 10.231c-.04"),
            ("go.dev", "go", "M1.811 10.231c-.04"),
            ("goproxy.io", "go", "M1.811 10.231c-.04"),
            ("pypi.org", "python", "M14.25.18l.9.2.73."),
            ("pythonhosted.org", "python", "M14.25.18l.9.2.73."),
            ("googleapis.com", "google", "M12.48 10.92v3.28h"),
            ("google.com", "google", "M12.48 10.92v3.28h"),
            ("gcr.io", "google", "M12.48 10.92v3.28h"),
            ("pkg.dev", "google", "M12.48 10.92v3.28h"),
            ("bazel.build", "bazel", "m11.7473 23.8198-5"),
            ("k8s.io", "kubernetes", "M10.204 14.35l.007"),
            ("kubernetes.io", "kubernetes", "M10.204 14.35l.007"),
            ("codeberg.org", "codeberg", "M11.955.49A12 12 0"),
            ("amazonaws.com", "aws", "M6.763 10.036c0 .2"),
            ("ecr.aws", "aws", "M6.763 10.036c0 .2"),
            ("awsstatic.com", "aws", "M6.763 10.036c0 .2"),
            ("aws.amazon.com", "aws", "M6.763 10.036c0 .2"),
            ("cloudfront.net", "aws", "M6.763 10.036c0 .2"),
            ("debian.org", "debian", "M13.88 12.685c-.4 "),
            ("deb.debian.org", "debian", "M13.88 12.685c-.4 "),
            ("ubuntu.com", "ubuntu", "M17.61.455a3.41 3."),
            ("archive.ubuntu.com", "ubuntu", "M17.61.455a3.41 3."),
            ("alpinelinux.org", "alpine", "M5.998 1.607L0 12l"),
            ("dl-cdn.alpinelinux.org", "alpine", "M5.998 1.607L0 12l"),
            ("fedoraproject.org", "fedora", "M12.001 0C5.376 0 "),
            ("mirrors.fedoraproject.org", "fedora", "M12.001 0C5.376 0 "),
            ("redhat.com", "redhat", "M16.009 13.386c1.5"),
            ("redhat.io", "redhat", "M16.009 13.386c1.5"),
            ("registry.redhat.io", "redhat", "M16.009 13.386c1.5"),
            ("helm.sh", "helm", "M12.337 0c-.475 0-"),
            ("nodejs.org", "nodejs", "M11.998,24c-0.321,"),
            ("rust-lang.org", "rust", "M23.8346 11.7033l-"),
            ("crates.io", "rust", "M23.8346 11.7033l-"),
            ("gitlab.com", "gitlab", "m23.6004 9.5927-.0"),
        ] {
            let html = rendered(host);
            assert!(
                html.contains(&format!("data-host-icon=\"brand-{name}\"")),
                "{host}"
            );
            // The prefix is pinned here, so swapped paths in brands.json fail.
            assert!(html.contains(&format!("d=\"{prefix}")), "{host}");
        }
    }

    #[test]
    fn the_provider_file_is_well_formed() {
        let mut names = std::collections::HashSet::new();
        let mut domains = std::collections::HashSet::new();
        for brand in BRANDS.iter() {
            assert!(names.insert(&brand.name), "duplicate {}", brand.name);
            assert!(
                (1..=3).contains(&brand.monogram.chars().count()),
                "{}",
                brand.name
            );
            for domain in &brand.domains {
                assert_eq!(
                    &normalize_host(domain),
                    domain,
                    "{domain} is not normalized"
                );
                assert!(!domain.starts_with("*.") && !domain.is_empty(), "{domain}");
                assert!(domains.insert(domain), "duplicate domain {domain}");
            }
            if let Some(path) = &brand.path {
                assert!(
                    !path.is_empty()
                        && path
                            .chars()
                            .all(|c| c.is_ascii_digit() || "MmLlHhVvCcSsQqTtAaZzeE .,-".contains(c)),
                    "{}",
                    brand.name
                );
            }
        }
        let github = BRANDS.iter().find(|brand| brand.name == "github");
        assert!(github.is_some_and(|brand| brand.path.is_some()));
    }

    #[test]
    fn aws_covers_its_subdomains_but_not_the_amazon_shop() {
        let s3 = "gitlabcommunity-eu1-eu-west-1-prod-packages.s3.eu-west-1.amazonaws.com";
        assert!(rendered(s3).contains("data-host-icon=\"brand-aws\""));
        assert!(rendered("console.aws.amazon.com").contains("data-host-icon=\"brand-aws\""));
        for host in ["amazon.com", "www.amazon.com"] {
            assert!(
                rendered(host).contains("data-host-icon=\"globe\""),
                "{host}"
            );
        }
    }

    #[test]
    fn project_host_icons_have_the_same_size_as_provider_glyphs() {
        let forgejo = [project(
            "https://git.example.internal/o/r.git",
            Some("https://jira.example.com/DF"),
            true,
        )];
        let github = [project("https://code.example.org/o/r.git", None, true)];
        let plain = [project("https://code.example.org/o/r.git", None, false)];
        let tiles = [
            with_projects("github", &plain, "code.example.org"),
            rendered("10.0.0.1"),
            with_projects("forgejo", &forgejo, "git.example.internal"),
            with_projects("forgejo", &forgejo, "jira.example.com"),
            with_projects("github", &github, "code.example.org"),
            rendered("github.com"),
            rendered("example.org"),
        ];
        // Role icons wrap their svg in a span, so the size must reach nested svgs.
        for html in tiles {
            assert!(html.contains("[&amp;_svg]:size-4"), "{html}");
            assert!(!html.contains("[&amp;&gt;svg]"), "{html}");
        }
    }

    #[test]
    fn codeberg_shows_the_forgejo_icon_for_a_served_project_and_its_glyph_otherwise() {
        let projects = [project("https://codeberg.org/o/r.git", None, true)];
        let served = with_projects("forgejo", &projects, "codeberg.org");
        assert!(
            served.contains("data-host-icon=\"forge-forgejo\""),
            "{served}"
        );
        assert!(rendered("codeberg.org").contains("data-host-icon=\"brand-codeberg\""));
    }

    #[test]
    fn every_brand_glyph_is_a_filled_path() {
        for brand in BRANDS.iter() {
            let path = brand.path.as_deref().expect("every provider has a glyph");
            let svg = glyph_svg(path).into_string();
            assert!(
                svg.contains("fill=\"currentColor\" stroke=\"none\""),
                "{}",
                brand.name
            );
        }
    }

    #[test]
    fn a_gitlab_project_keeps_the_git_icon_on_its_repository_host() {
        let projects = [project("https://gitlab.com/o/r.git", None, true)];
        let no_credential = [project("https://gitlab.com/o/r.git", None, false)];
        let plain = with_projects("github", &no_credential, "gitlab.com");
        assert!(plain.contains("data-host-icon=\"forge-git\""), "{plain}");
        let unrouted =
            ProjectHosts::from_projects(&df_core::routing::ForgeRoutes::default(), &projects);
        let html = host_icon("gitlab.com", HostIconSize::Table, &unrouted).into_string();
        assert!(html.contains("data-host-icon=\"forge-git\""), "{html}");
        assert!(rendered("gitlab.com").contains("data-host-icon=\"brand-gitlab\""));
    }

    #[test]
    fn a_provider_without_a_glyph_shows_its_monogram() {
        let brands = [Brand {
            domains: vec!["example.dev".into()],
            name: "example".into(),
            monogram: "Ex".into(),
            tone: Tone::Queued,
            path: None,
        }];
        let brand = brand_in(&brands, "api.example.dev").unwrap();
        let html = brand_tile(brand, "size-6", "text-[10px]").into_string();
        assert!(
            html.contains("data-host-icon=\"Ex\"") && html.contains(">Ex<"),
            "{html}"
        );
    }

    #[test]
    fn a_wildcard_entry_uses_its_suffix() {
        assert!(rendered("*.github.com").contains("data-host-icon=\"brand-github\""));
        assert!(rendered("*.pypi.org").contains("data-host-icon=\"brand-python\""));
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
            assert!(
                rendered(host).contains("data-host-icon=\"brand-github\""),
                "{host}"
            );
        }
        assert!(rendered("raw.githubusercontent.com").contains("data-host-icon=\"brand-github\""));
        assert!(rendered("files.pythonhosted.org").contains("data-host-icon=\"brand-python\""));
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
        for host in ["example.org", "", "localhost", "[[::1", "::1]", "[::1"] {
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
