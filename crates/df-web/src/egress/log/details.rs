//! The request details panel: reason explanations and the panel markup that Alpine fills from a
//! row's `data-flow`.

use maud::{Markup, html};

use crate::components::{Icon, icon};

/// Fixed text of the panel about what it cannot show.
const TLS_NOTE: &str = "Requests inside TLS tunnels are not inspected.";

/// One sentence per reason code of `FlowEvent.reason` (`gateway.proto`); `None` for codes this
/// version does not know.
pub(super) fn reason_explanation(code: &str) -> Option<&'static str> {
    Some(match code {
        "no-snapshot" => "The gateway has not received the egress policy yet.",
        "unknown-source" => "The connection does not come from a running sandbox.",
        "ip-literal" => "The destination is an IP address; only host names can be allowed.",
        "not-allowlisted" => {
            "No egress rule of the run's project or instance matches this host and port."
        }
        "private-destination" => "The host name resolves to a private or internal address range.",
        "sni-mismatch" => "The TLS handshake names no host or another host than the one requested.",
        "removed-from-allowlist" => {
            "The rule was removed while the connection was open, so it was closed."
        }
        "gateway-shutdown" => {
            "The gateway stopped (for example during a rollout) and closed the connection."
        }
        "resolve-failed" => "The host name could not be resolved.",
        "upstream-failed" => "Connecting to or talking with the destination failed.",
        "upstream-timeout" => "The destination did not answer in time.",
        "tunnel-error" => "The tunnel ended with an I/O error.",
        _ => return None,
    })
}

fn row(label: &str, value: &str) -> Markup {
    html! {
        dt class="text-faint" { (label) }
        dd class="min-w-0 break-words font-mono text-[13px]" "x-text"=(value) {}
    }
}

/// The panel. It lives in the Alpine scope of the page (`TRAFFIC_STATE`): `flow` is the parsed
/// `data-flow` of the row that was opened, `close()` closes it again.
pub(super) fn panel() -> Markup {
    html! {
        aside x-show="flow" x-cloak "x-on:keydown.escape.window"="flow && close()" role="dialog" aria-labelledby="flow-title"
            class="fixed inset-y-0 right-0 z-40 flex w-full max-w-[500px] flex-col overflow-y-auto border-l border-line bg-panel shadow-xl" {
            header class="flex items-center gap-3 border-b border-line px-5 py-4" {
                span x-html="flow && flow.icon" {}
                div class="min-w-0 flex-1" {
                    h2 id="flow-title" class="truncate font-mono text-[15px] font-semibold" "x-text"="flow && flow.destination" {}
                    p class="text-xs text-faint" { span "x-text"="flow && flow.time" {} " UTC" }
                }
                button type="button" "x-on:click"="close()" "x-ref"="closeButton" aria-label="Close request details" title="Close"
                    class="inline-flex size-9 shrink-0 items-center justify-center rounded-lg text-muted hover:bg-sunk hover:text-ink" {
                    (icon(Icon::Close))
                }
            }
            div class="flex flex-wrap items-center gap-2 border-b border-line bg-sunk px-5 py-3 text-sm" {
                span x-html="flow && flow.pill" {}
                span class="font-mono text-[13px]" "x-text"="flow && flow.method" {}
                span class="text-muted" "x-text"="flow && flow.status" {}
                span class="ml-auto font-mono text-[13px] text-muted" "x-text"="flow && flow.duration" {}
            }
            section class="space-y-2 border-b border-line px-5 py-4" {
                h3 class="text-xs font-medium uppercase tracking-wide text-faint" { "Connection" }
                dl class="grid grid-cols-[110px_minmax(0,1fr)] gap-x-3 gap-y-2 text-[13px]" {
                    (row("Sandbox", "flow && flow.sandbox"))
                    (row("Task", "flow && flow.task"))
                    (row("Sent", "flow && flow.bytes_out"))
                    (row("Received", "flow && flow.bytes_in"))
                    (row("SNI", "flow && flow.sni"))
                    (row("Source", "flow && flow.source_ip"))
                    (row("Gateway pod", "flow && flow.gateway_pod"))
                }
            }
            section x-show="flow && flow.reason" class="space-y-2 border-b border-line px-5 py-4" {
                h3 class="text-xs font-medium uppercase tracking-wide text-faint" { "Reason" }
                p class="font-mono text-[13px]" "x-text"="flow && flow.reason" {}
                p class="text-sm text-muted" "x-text"="flow && flow.explanation" {}
            }
            section class="space-y-2 border-b border-line px-5 py-4" {
                h3 class="text-xs font-medium uppercase tracking-wide text-faint" { "Matched rule" }
                template x-if="flow && flow.rule" {
                    a ":href"="flow.rule_href" class="font-mono text-[13px] text-accent-text underline" "x-text"="flow.rule" {}
                }
                p x-show="flow && !flow.rule" class="text-sm text-muted" { "No rule matched" }
            }
            p class="px-5 py-4 text-sm text-faint" { (TLS_NOTE) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The codes documented at `FlowEvent.reason` in `gateway.proto`.
    const DOCUMENTED: [&str; 12] = [
        "no-snapshot",
        "unknown-source",
        "ip-literal",
        "not-allowlisted",
        "private-destination",
        "sni-mismatch",
        "removed-from-allowlist",
        "gateway-shutdown",
        "resolve-failed",
        "upstream-failed",
        "upstream-timeout",
        "tunnel-error",
    ];

    #[test]
    fn every_documented_reason_has_an_explanation() {
        for code in DOCUMENTED {
            assert!(reason_explanation(code).is_some(), "{code}");
        }
        assert_eq!(reason_explanation("from-the-future"), None);
        assert_eq!(reason_explanation(""), None);
    }

    #[test]
    fn the_panel_is_a_named_dialog_with_the_tls_note_and_no_request_content() {
        let html = panel().into_string();
        assert!(
            html.contains("role=\"dialog\"") && html.contains("aria-labelledby=\"flow-title\"")
        );
        assert!(html.contains("id=\"flow-title\""));
        assert!(html.contains("Requests inside TLS tunnels are not inspected."));
        assert!(html.contains("No rule matched") && html.contains("Matched rule"));
        for shown in [
            "Sandbox",
            "Task",
            "Sent",
            "Received",
            "SNI",
            "Source",
            "Gateway pod",
            "Reason",
        ] {
            assert!(html.contains(&format!(">{shown}<")), "{shown}");
        }
        let lower = html.to_ascii_lowercase();
        for absent in ["url", "query", "headers", "content-type", "content type"] {
            assert!(!lower.contains(absent), "{absent}");
        }
        assert!(html.contains("aria-label=\"Close request details\""));
    }
}
