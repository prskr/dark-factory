//! Rows of the events table and the data the details panel reads from them.

use chrono::DateTime;
use df_proto::gateway::v1::{FlowEvent, Phase};
use maud::{Markup, html};
use serde_json::json;

use super::details::reason_explanation;
use super::filter::is_blocked;
use crate::components::{HostIconSize, Icon, ProjectHosts, Tone, host_icon, icon, tone_badge};

const CELL: &str = "px-3 py-3";

/// How a connection ended up, in the words of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    Allowed,
    Blocked,
    /// The tunnel is still open; counts as allowed.
    Open,
}

impl Outcome {
    pub(super) fn of(event: &FlowEvent) -> Self {
        if is_blocked(event) {
            Self::Blocked
        } else if event.phase() == Phase::Opened {
            Self::Open
        } else {
            Self::Allowed
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Allowed => "Allowed",
            Self::Blocked => "Blocked",
            Self::Open => "Open",
        }
    }

    const fn tone(self) -> Tone {
        match self {
            Self::Allowed => Tone::Completed,
            Self::Blocked => Tone::Failed,
            Self::Open => Tone::Running,
        }
    }

    /// The pill of the verdict column and of the details panel.
    pub(super) fn pill(self) -> Markup {
        tone_badge(self.tone(), self.label())
    }
}

pub(super) fn time_label(event: &FlowEvent) -> String {
    event
        .time
        .as_ref()
        .and_then(|t| DateTime::from_timestamp(t.seconds, u32::try_from(t.nanos).unwrap_or(0)))
        .map_or_else(String::new, |time| time.format("%H:%M:%S").to_string())
}

/// The element id of a flow's row: unique per gateway pod and flow. The pod name is hex encoded
/// so that any name makes a valid id.
pub(super) fn row_id(event: &FlowEvent) -> String {
    format!("flow-{}-{}", hex::encode(&event.gateway_pod), event.flow_id)
}

/// The sandbox of an event: the run id, or the source address if the gateway knows no run.
pub(super) fn sandbox_key(event: &FlowEvent) -> &str {
    if event.run_id.is_empty() {
        &event.source_ip
    } else {
        &event.run_id
    }
}

/// First 8 characters of the run id, or the source address of an unknown source.
pub(super) fn sandbox_label(event: &FlowEvent) -> String {
    sandbox_key(event).chars().take(8).collect()
}

/// The task key, or a note for connections from an unknown source.
pub(super) fn task_label(event: &FlowEvent) -> String {
    if !event.task_key.is_empty() {
        event.task_key.clone()
    } else if event.run_id.is_empty() {
        "unknown source".to_string()
    } else {
        String::new()
    }
}

/// How long the connection lasted: `12 ms`, `3.4 s`, `2:05`. Empty while it is open.
pub(super) fn duration_label(event: &FlowEvent) -> String {
    let Some(duration) = event
        .duration
        .as_ref()
        .filter(|_| event.phase() == Phase::Closed)
    else {
        return String::new();
    };
    // gRPC values are not trusted to be sane: clamp instead of overflowing or showing a minus.
    let millis = duration
        .seconds
        .saturating_mul(1000)
        .saturating_add(i64::from(duration.nanos) / 1_000_000)
        .max(0);
    match millis {
        ..1000 => format!("{millis} ms"),
        1000..60_000 => format!("{:.1} s", millis as f64 / 1000.0),
        _ => format!("{}:{:02}", millis / 60_000, millis / 1000 % 60),
    }
}

/// `512 B`, `1.5 KB`, `3.2 MB`.
pub(super) fn bytes_label(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let value = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if value < KB * KB {
        format!("{:.1} KB", value / KB)
    } else if value < KB * KB * KB {
        format!("{:.1} MB", value / (KB * KB))
    } else {
        format!("{:.1} GB", value / (KB * KB * KB))
    }
}

/// Link to the rules page with the rules of the matched pattern highlighted; `None` if no rule
/// matched.
pub(super) fn rule_href(event: &FlowEvent) -> Option<String> {
    (!event.matched_rule.is_empty()).then(|| {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.append_pair(
            "highlight",
            &format!("{}:{}", event.matched_rule, event.port),
        );
        format!("/egress/allowlist?{}", query.finish())
    })
}

/// The event as the details panel shows it. Everything is text, formatted here; the two pieces of
/// markup (`icon`, `pill`) are built from fixed tables, never from the event's strings.
fn flow_json(event: &FlowEvent, target: &str, projects: &ProjectHosts) -> String {
    let outcome = Outcome::of(event);
    let status = if event.http_status != 0 {
        event.http_status.to_string()
    } else {
        outcome.label().to_string()
    };
    let closed = event.phase() == Phase::Closed;
    json!({
        "destination": target,
        "icon": host_icon(&event.host, HostIconSize::Panel, projects).into_string(),
        "pill": outcome.pill().into_string(),
        "time": time_label(event),
        "sandbox": sandbox_label(event),
        "task": task_label(event),
        "method": event.method,
        "status": status,
        "duration": duration_label(event),
        "bytes_out": if closed { bytes_label(event.bytes_out) } else { String::new() },
        "bytes_in": if closed { bytes_label(event.bytes_in) } else { String::new() },
        "sni": event.sni,
        "source_ip": event.source_ip,
        "gateway_pod": event.gateway_pod,
        "reason": event.reason,
        "explanation": reason_explanation(&event.reason).unwrap_or(""),
        "rule": event.matched_rule,
        "rule_href": rule_href(event),
    })
    .to_string()
}

/// A row of the events table. In a narrow panel (container query, `@[680px]` is the wide
/// variant) the verdict column is hidden and the verdict shows as the edge of the row.
pub(super) fn row(event: &FlowEvent, projects: &ProjectHosts) -> Markup {
    if event.gap {
        return html! {
            tr class="border-t border-line" {
                td colspan="5" class="px-5 py-2 text-center text-sm text-faint" {
                    "Events are missing: the page could not keep up."
                }
            }
        };
    }
    let outcome = Outcome::of(event);
    let id = (event.flow_id != 0).then(|| row_id(event));
    let time = time_label(event);
    let target = format!("{}:{}", event.host, event.port);
    let (edge, tint) = match outcome {
        Outcome::Blocked => (
            "border-l-4 border-dashed border-danger",
            " bg-danger-soft/40 @[680px]:bg-transparent",
        ),
        Outcome::Allowed | Outcome::Open => {
            ("border-l-4 border-solid border-status-completed-fg", "")
        }
    };
    let rule = rule_href(event).filter(|_| outcome != Outcome::Blocked);
    html! {
        tr id=[id.as_deref()] class={"border-t border-line" (tint)} data-flow=(flow_json(event, &target, projects)) data-outcome=(outcome.label().to_ascii_lowercase()) {
            td class={"font-mono text-[13px] text-muted " (CELL) " " (edge) " @[680px]:border-l-0"} { (time) }
            td class=(CELL) {
                span class="font-mono text-[13px]" { (sandbox_label(event)) }
                " " span class="text-xs text-faint" { (task_label(event)) }
            }
            td class=(CELL) {
                span class="flex items-center gap-2" {
                    (host_icon(&event.host, HostIconSize::Table, projects))
                    span class="min-w-0 truncate font-mono text-[13px]" { (target) }
                    span class="sr-only @[680px]:hidden" { (outcome.label()) }
                }
            }
            td class={"hidden @[680px]:table-cell " (CELL)} {
                (outcome.pill())
                @if !event.reason.is_empty() { " " span class="text-xs text-faint" { (event.reason) } }
            }
            td class="py-2 pr-4 pl-3 text-right" {
                // Wide panel: inline actions. The slot is where later actions (allow) go.
                div class="hidden items-center justify-end gap-1 @[680px]:flex" data-slot="row-actions" {
                    (details_button(&target, &time, "inline-flex h-9 items-center rounded-lg px-3 text-sm text-muted hover:bg-sunk hover:text-ink", html! { "Details" }))
                    @if let Some(href) = &rule {
                        a href=(href) class="inline-flex h-9 items-center rounded-lg px-3 text-sm text-accent-text hover:bg-sunk" { "Rule" }
                    }
                }
                // Narrow panel: one menu.
                div class="relative @[680px]:hidden" x-data="{ menu: false }" "x-on:keydown.escape"="menu = false" "x-on:click.outside"="menu = false" {
                    button type="button" aria-haspopup="true" ":aria-expanded"="menu" "x-on:click"="menu = !menu"
                        aria-label={"Actions for " (target) " at " (time)}
                        class="inline-flex size-9 items-center justify-center rounded-lg text-muted hover:bg-sunk hover:text-ink" {
                        (icon(Icon::Ellipsis))
                    }
                    div x-show="menu" x-cloak class="absolute right-0 z-20 mt-1 w-44 rounded-xl border border-line bg-panel p-1 text-left shadow-lg" data-slot="row-menu" {
                        (details_button(&target, &time, "flex min-h-10 w-full items-center rounded-lg px-3 text-sm hover:bg-sunk", html! { "Request details" }))
                        @if let Some(href) = &rule {
                            a href=(href) class="flex min-h-10 items-center rounded-lg px-3 text-sm hover:bg-sunk" { "Rule" }
                        }
                    }
                }
            }
        }
    }
}

fn details_button(target: &str, time: &str, class: &str, label: Markup) -> Markup {
    html! {
        button type="button" "x-on:click"="openDetails($el)" aria-label={"Request details for " (target) " at " (time)} class=(class) { (label) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use df_proto::gateway::v1::Verdict;
    use df_proto::pbjson_types::{Duration, Timestamp};

    /// The row without project hosts.
    fn row(event: &FlowEvent) -> Markup {
        super::row(event, &ProjectHosts::default())
    }

    fn event(verdict: Verdict, phase: Phase) -> FlowEvent {
        FlowEvent {
            time: Some(Timestamp {
                seconds: 1_700_000_000,
                nanos: 0,
            }),
            run_id: "a4c2e7f01234".into(),
            task_key: "DF-7".into(),
            host: "203.0.113.42".into(),
            port: 22,
            method: "CONNECT".into(),
            verdict: verdict.into(),
            phase: phase.into(),
            gateway_pod: "gw-0".into(),
            flow_id: 3,
            ..Default::default()
        }
    }

    #[test]
    fn outcomes_follow_verdict_and_phase() {
        assert_eq!(
            Outcome::of(&event(Verdict::Denied, Phase::Closed)),
            Outcome::Blocked
        );
        assert_eq!(
            Outcome::of(&event(Verdict::Allowed, Phase::Opened)),
            Outcome::Open
        );
        assert_eq!(
            Outcome::of(&event(Verdict::Allowed, Phase::Closed)),
            Outcome::Allowed
        );
        // A denied event is blocked whatever its phase says.
        assert_eq!(
            Outcome::of(&event(Verdict::Denied, Phase::Opened)),
            Outcome::Blocked
        );
    }

    #[test]
    fn a_blocked_row_shows_sandbox_task_destination_and_pill() {
        let html = row(&event(Verdict::Denied, Phase::Closed)).into_string();
        assert!(
            html.contains(">a4c2e7f0<") && html.contains(">DF-7<"),
            "{html}"
        );
        assert!(
            html.contains(">203.0.113.42:22<") && html.contains(">Blocked<"),
            "{html}"
        );
        assert!(html.contains("border-dashed border-danger"), "{html}");
        // The whole row is tinted, in the narrow variant only.
        assert!(
            html.contains("border-line bg-danger-soft/40 @[680px]:bg-transparent\" data-flow"),
            "{html}"
        );
        let first_cell = &html[html.find("<td").unwrap()..];
        assert!(!first_cell[..first_cell.find("</td>").unwrap()].contains("bg-danger-soft"));
        assert!(
            !html.contains(">Rule<"),
            "blocked rows offer only the details: {html}"
        );
        assert!(html.contains("data-outcome=\"blocked\""));
    }

    #[test]
    fn an_allowed_row_links_to_its_rule_and_an_open_one_says_open() {
        let mut allowed = event(Verdict::Allowed, Phase::Closed);
        allowed.matched_rule = "*.crates.io".into();
        allowed.port = 443;
        let html = row(&allowed).into_string();
        assert!(
            html.contains("href=\"/egress/allowlist?highlight=*.crates.io%3A443\""),
            "{html}"
        );
        assert!(html.contains(">Allowed<") && html.contains("border-status-completed-fg"));

        let no_rule = row(&event(Verdict::Allowed, Phase::Closed)).into_string();
        assert!(!no_rule.contains(">Rule<"));
        let open = row(&event(Verdict::Allowed, Phase::Opened)).into_string();
        assert!(
            open.contains(">Open<") && !open.contains(">Allowed<"),
            "{open}"
        );
    }

    #[test]
    fn both_table_variants_are_in_the_markup_and_the_menu_button_is_named() {
        let html = row(&event(Verdict::Denied, Phase::Closed)).into_string();
        // Wide: verdict column and inline actions appear from 680 px.
        assert!(html.contains("hidden @[680px]:table-cell"), "{html}");
        assert!(html.contains("hidden items-center justify-end gap-1 @[680px]:flex"));
        // Narrow: row edge and menu, both gone from 680 px.
        assert!(html.contains("@[680px]:border-l-0") && html.contains("relative @[680px]:hidden"));
        assert!(
            html.contains("sr-only @[680px]:hidden"),
            "verdict for assistive technology"
        );
        assert!(
            html.contains("aria-label=\"Actions for 203.0.113.42:22 at 22:13:20\""),
            "{html}"
        );
        assert!(html.contains("aria-label=\"Request details for 203.0.113.42:22 at 22:13:20\""));
    }

    #[test]
    fn the_row_carries_the_event_as_escaped_json_for_the_panel() {
        let mut closed = event(Verdict::Allowed, Phase::Closed);
        closed.host = "api.anthropic.com".into();
        closed.port = 443;
        closed.sni = "api.anthropic.com".into();
        closed.matched_rule = "api.anthropic.com".into();
        closed.bytes_out = 1536;
        closed.bytes_in = 7;
        closed.duration = Some(Duration {
            seconds: 2,
            nanos: 500_000_000,
        });
        let html = row(&closed).into_string();
        let start = html.find("data-flow=\"").unwrap() + "data-flow=\"".len();
        let attr = &html[start..start + html[start..].find('"').unwrap()];
        assert!(
            !attr.contains('<') && !attr.contains('"'),
            "escaped: {attr}"
        );
        let unescaped = attr
            .replace("&quot;", "\"")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&");
        let json: serde_json::Value = serde_json::from_str(&unescaped).unwrap();
        assert_eq!(json["method"], "CONNECT");
        assert_eq!(json["destination"], "api.anthropic.com:443");
        assert_eq!(json["bytes_out"], "1.5 KB");
        assert_eq!(json["bytes_in"], "7 B");
        assert_eq!(json["duration"], "2.5 s");
        assert_eq!(json["rule"], "api.anthropic.com");
        assert_eq!(
            json["rule_href"],
            "/egress/allowlist?highlight=api.anthropic.com%3A443"
        );
        assert_eq!(json["status"], "Allowed");
        assert!(json["pill"].as_str().unwrap().contains("Allowed"));
    }

    #[test]
    fn a_denied_event_in_the_panel_data_has_a_reason_and_no_rule() {
        let mut denied = event(Verdict::Denied, Phase::Closed);
        denied.reason = "not-allowlisted".into();
        let json: serde_json::Value =
            serde_json::from_str(&flow_json(&denied, "x:22", &ProjectHosts::default())).unwrap();
        assert_eq!(json["reason"], "not-allowlisted");
        assert!(
            json["explanation"]
                .as_str()
                .unwrap()
                .contains("No egress rule")
        );
        assert_eq!(json["rule"], "");
        assert!(json["rule_href"].is_null());
        assert_eq!(json["status"], "Blocked");
        // An unknown code is shown as it is, without explanation.
        denied.reason = "from-the-future".into();
        let json: serde_json::Value =
            serde_json::from_str(&flow_json(&denied, "x:22", &ProjectHosts::default())).unwrap();
        assert_eq!(json["explanation"], "");
    }

    #[test]
    fn http_status_replaces_the_verdict_as_status_and_open_tunnels_have_no_totals() {
        let mut plain = event(Verdict::Allowed, Phase::Closed);
        plain.http_status = 200;
        plain.method = "GET".into();
        let json: serde_json::Value =
            serde_json::from_str(&flow_json(&plain, "x:22", &ProjectHosts::default())).unwrap();
        assert_eq!(json["status"], "200");
        let mut open = event(Verdict::Allowed, Phase::Opened);
        open.bytes_out = 99;
        let json: serde_json::Value =
            serde_json::from_str(&flow_json(&open, "x:22", &ProjectHosts::default())).unwrap();
        assert_eq!(json["bytes_out"], "");
        assert_eq!(json["duration"], "");
        assert_eq!(json["status"], "Open");
    }

    #[test]
    fn unknown_sources_show_their_address() {
        let mut unknown = event(Verdict::Denied, Phase::Closed);
        unknown.run_id.clear();
        unknown.task_key.clear();
        unknown.source_ip = "10.1.2.3".into();
        let html = row(&unknown).into_string();
        assert!(
            html.contains(">10.1.2.3<") && html.contains(">unknown source<"),
            "{html}"
        );
    }

    #[test]
    fn a_gap_row_spans_the_table() {
        let html = row(&FlowEvent {
            gap: true,
            ..Default::default()
        })
        .into_string();
        assert!(html.contains("Events are missing") && html.contains("colspan=\"5\""));
    }

    #[test]
    fn byte_counts_are_short() {
        assert_eq!(bytes_label(0), "0 B");
        assert_eq!(bytes_label(1023), "1023 B");
        assert_eq!(bytes_label(1024), "1.0 KB");
        assert_eq!(bytes_label(5 * 1024 * 1024), "5.0 MB");
        assert!(bytes_label(u64::MAX).ends_with("GB"));
    }
}
