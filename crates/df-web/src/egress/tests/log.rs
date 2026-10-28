//! The log page, its stream, filters and graph.

use axum::http::StatusCode;
use df_proto::gateway::v1::FlowEvent;

use super::{allowed, app_with_egress, denied, filter_from_query, inner, live, new_viewer, part};
use crate::egress::log::{Filter, VerdictFilter};
use crate::testkit::{SseReader, TestApp, header_str, seed, text};

#[test]
fn new_rows_come_newest_first_and_the_graph_marks_the_blocked_destination() {
    let batch = [
        denied("statsig.anthropic.com", "DF", 1),
        allowed("api.anthropic.com", "DF", 2),
        denied("statsig.anthropic.com", "DF", 3),
        denied("evil.example.com", "DF", 5),
    ];
    let body = live(Filter::default(), &batch).unwrap();
    let rows = part(&body, "#flows tbody").unwrap();
    assert!(body.contains("afterbegin"));
    assert!(
        rows.find("evil.example.com").unwrap() < rows.find("api.anthropic.com").unwrap(),
        "{rows}"
    );
    let graph = part(&body, "#graph").unwrap();
    assert_eq!(graph.matches("data-blocked=\"true\"").count(), 2, "{graph}");
    assert!(graph.contains(">statsig.anthropic.com<") && graph.contains("class=\"fill-danger\""));
    assert!(!body.contains("denied-hosts"), "the summary is gone");
}

#[test]
fn a_repeatedly_denied_domain_is_a_blocked_destination_and_a_blocked_row() {
    // Scenario "Missing domain".
    let events: Vec<_> = (1..=3)
        .map(|i| denied("statsig.anthropic.com", "DF", i))
        .collect();
    let mut viewer = new_viewer(Filter {
        verdict: Some(VerdictFilter::Blocked),
        ..Default::default()
    });
    let body = viewer.backlog_message(&events);
    assert_eq!(
        part(&body, "#flows tbody")
            .unwrap()
            .matches("data-outcome=\"blocked\"")
            .count(),
        3
    );
    let graph = part(&body, "#graph").unwrap();
    assert_eq!(
        graph.matches("data-blocked=\"true\"").count(),
        1,
        "one destination: {graph}"
    );
    assert_eq!(part(&body, "#count-blocked"), Some("3"));
}

#[test]
fn segment_counts_ignore_the_verdict_filter() {
    // Scenario "Segment counts ignore the verdict": pypi, 3 blocked and 12 allowed.
    let mut events: Vec<_> = (0..3).map(|i| denied("pypi.org", "DF", i)).collect();
    events.extend((0..12).map(|i| allowed("pypi.org", "DF", 10 + i)));
    events.push(allowed("github.com", "DF", 30));
    for verdict in [
        None,
        Some(VerdictFilter::Blocked),
        Some(VerdictFilter::Allowed),
    ] {
        let filter = Filter {
            q: "pypi".into(),
            verdict,
            ..Default::default()
        };
        let body = new_viewer(filter).backlog_message(&events);
        assert_eq!(part(&body, "#count-all"), Some("15"), "{verdict:?}");
        assert_eq!(part(&body, "#count-blocked"), Some("3"), "{verdict:?}");
        assert_eq!(part(&body, "#count-allowed"), Some("12"), "{verdict:?}");
    }
    let body = new_viewer(Filter {
        q: "pypi".into(),
        verdict: Some(VerdictFilter::Blocked),
        ..Default::default()
    })
    .backlog_message(&events);
    assert_eq!(part(&body, "#event-count"), Some("3 of 16"));
}

#[test]
fn filters_limit_rows_but_not_the_graph() {
    let batch = [
        denied("a.example.com", "DF", 1),
        allowed("b.example.com", "DF", 2),
        denied("c.example.com", "WEB", 3),
    ];
    let filter = Filter {
        denied: "1".into(),
        task: "DF-1".into(),
        ..Default::default()
    };
    let body = live(filter, &batch).unwrap();
    let rows = part(&body, "#flows tbody").unwrap();
    assert!(rows.contains("a.example.com"));
    assert!(
        !rows.contains("b.example.com") && !rows.contains("c.example.com"),
        "{rows}"
    );
    // Scenario "Filtered out": the others are in the graph, dimmed.
    let graph = part(&body, "#graph").unwrap();
    assert!(graph.contains(">b.example.com<") && graph.contains(">c.example.com<"));
    assert_eq!(
        graph.matches("opacity-30").count(),
        4,
        "2 nodes and 2 edges of the other two: {graph}"
    );
    assert_eq!(part(&body, "#event-count"), Some("1 of 3"));

    // Nothing passes: no row, but the counts and the graph still follow.
    let nothing = Filter {
        q: "nope".into(),
        ..Default::default()
    };
    let body = live(nothing, &batch).unwrap();
    assert!(part(&body, "#flows tbody").is_none());
    assert_eq!(part(&body, "#event-count"), Some("0 of 3"));
    assert!(
        part(&body, "#flows-empty")
            .unwrap()
            .contains("No events match these filters")
    );
    assert!(
        part(&body, "#flows-empty")
            .unwrap()
            .contains("Clear filters")
    );
}

#[test]
fn a_gap_is_a_row_that_changes_neither_graph_nor_counts() {
    let gap = FlowEvent {
        gap: true,
        ..Default::default()
    };
    let mut viewer = new_viewer(Filter {
        q: "never".into(),
        ..Default::default()
    });
    let body = viewer.backlog_message(&[]);
    assert_eq!(part(&body, "#event-count"), Some("0 of 0"));
    let message = viewer.live_message(&[gap]).unwrap();
    assert!(message.contains("Events are missing"), "{message}");
    assert!(
        part(&message, "#graph").is_none() && part(&message, "#event-count").is_none(),
        "{message}"
    );
}

#[test]
fn the_filter_query_carries_only_set_fields_and_maps_old_parameters() {
    assert_eq!(Filter::default().query(), "");
    // Old link: `denied=1` is Blocked, `host` the search; the dropped `project` and `sandbox`
    // filters are ignored, so the link still opens.
    let old: Filter = filter_from_query("denied=1&project=DF&sandbox=a4c2&host=a+b");
    assert_eq!(old.query(), "?verdict=blocked&q=a+b");
    assert_eq!(old.verdict_filter().param(), "blocked");
    assert_eq!(old.advanced_count(), 0);
    // Shared link.
    let shared: Filter = filter_from_query("verdict=blocked&q=pypi&task=DF-7");
    assert_eq!(shared.query(), "?verdict=blocked&q=pypi&task=DF-7");
    assert_eq!(shared.advanced_count(), 1);
    // `all` and unknown values are the default.
    assert_eq!(filter_from_query("verdict=all").query(), "");
    assert_eq!(filter_from_query("verdict=bogus").query(), "");
    assert_eq!(filter_from_query("q=a&host=b").search(), "a");
}

#[tokio::test]
async fn a_repeatedly_denied_domain_appears_in_the_stream() {
    let events = vec![
        denied("statsig.anthropic.com", "DF", 1_700_000_001),
        denied("statsig.anthropic.com", "DF", 1_700_000_002),
        denied("statsig.anthropic.com", "DF", 1_700_000_003),
    ];
    let app = app_with_egress(events).await;
    let cookie = app.login().await;
    let response = app
        .get("/egress/log/events?verdict=blocked&task=DF-1", &cookie)
        .await;
    assert_eq!(header_str(&response, "content-type"), "text/event-stream");
    let frame = SseReader::new(response).next().await;
    let rows = part(&frame.data, "#flows tbody").unwrap();
    assert_eq!(
        rows.matches("data-outcome=\"blocked\"").count(),
        3,
        "{rows}"
    );
    assert!(
        part(&frame.data, "#graph")
            .unwrap()
            .contains("data-blocked=\"true\"")
    );
    assert_eq!(part(&frame.data, "#event-count"), Some("3 of 3"));
}

#[tokio::test]
async fn the_stream_is_unfiltered_so_the_graph_sees_everything() {
    let events = vec![denied("bad.com", "DF", 1), allowed("good.com", "WEB", 2)];
    let app = app_with_egress(events).await;
    let cookie = app.login().await;
    let frame = SseReader::new(app.get("/egress/log/events?task=DF-1", &cookie).await)
        .next()
        .await;
    assert!(part(&frame.data, "#graph").unwrap().contains(">good.com<"));
    assert!(
        !part(&frame.data, "#flows tbody")
            .unwrap()
            .contains("good.com")
    );
}

#[tokio::test]
async fn the_log_page_has_header_filter_bar_and_the_stream_with_its_filters() {
    let app = app_with_egress(vec![]).await;
    let persona = seed(&app.pool).await.persona;
    let cookie = app.login().await;
    let page = text(
        app.get(
            &format!("/egress/log?denied=1&task=DF-7&persona={persona}"),
            &cookie,
        )
        .await,
    )
    .await;
    assert!(
        page.contains("<title>Network traffic - Dark Factory</title>"),
        "{page}"
    );
    assert!(
        page.contains(
            &format!("hx-sse:connect=\"/egress/log/events?verdict=blocked&amp;task=DF-7&amp;persona={persona}\"")
        ),
        "{page}"
    );
    // Header: title, stream badge, pause, link to the rules.
    assert!(page.contains(">Network traffic</h1>") && page.contains("x-text=\"badge()\">Live<"));
    assert!(page.contains("togglePause()") && page.contains(">Pause<"));
    assert!(page.contains("<a href=\"/egress/allowlist\"") && page.contains(">Egress rules<"));
    assert!(
        page.contains("aria-label=\"Network traffic\""),
        "rail entry"
    );
    // Filter bar: before and outside `#traffic`, so typing never loses focus.
    let traffic = page.find("id=\"traffic\"").unwrap();
    for control in [
        "name=\"q\"",
        "name=\"verdict\"",
        "name=\"persona\"",
        "name=\"task\"",
        "Clear filters",
        "Advanced",
    ] {
        assert!(
            page.find(control).unwrap() < traffic,
            "{control} is outside #traffic"
        );
    }
    assert!(page.contains("hx-select=\"#traffic\"") && page.contains("hx-push-url=\"true\""));
    assert!(page.contains("hx-trigger=\"input changed delay:300ms, change\""));
    // Segments with counts; the selected one is checked; one advanced filter is in use besides project.
    assert!(
        page.contains("id=\"count-all\"")
            && page.contains("id=\"count-blocked\"")
            && page.contains("id=\"count-allowed\"")
    );
    assert!(page.contains("value=\"blocked\" checked"), "{page}");
    assert!(page.contains("value=\"DF-7\""));
    // Clear filters only shows while a filter is set.
    assert!(page.contains("dirty: true,") && page.contains("x-show=\"dirty\""));
    assert!(page.contains(">2</span>"), "advanced badge");
    // As in the design: one right-aligned row, Advanced opens an inline row below it.
    assert!(page.contains("id=\"filter-row\" class=\"flex flex-wrap items-center justify-end"));
    assert!(
        page.contains("aria-controls=\"adv-filters\"")
            && page.contains(":aria-expanded=\"advOpen\"")
    );
    assert!(page.contains("id=\"adv-filters\" class=\"flex flex-wrap items-center justify-end"));
    // The design's filter icon (Lucide `list-filter`) before the label.
    let advanced_button = &page[page.find("aria-controls=\"adv-filters\"").unwrap()..];
    assert!(
        advanced_button.find("M7 12h10").unwrap() < advanced_button.find("Advanced").unwrap(),
        "{advanced_button}"
    );
    // Persona by name; the dropped filters are gone.
    assert!(
        page.contains("<option value=\"\">All personas</option>"),
        "{page}"
    );
    assert!(
        page.contains(&format!(
            "<option value=\"{persona}\" selected>Linus</option>"
        )),
        "{page}"
    );
    assert!(!page.contains("name=\"sandbox\"") && !page.contains("name=\"project\""));
    // The advanced controls keep their own width, so label and control stay on one line.
    let advanced = &page[page.find("id=\"adv-filters\"").unwrap()..page.find("</form>").unwrap()];
    assert!(!advanced.contains("w-full"), "{advanced}");
    // Events panel and empty state.
    assert!(page.contains("Events · newest first") && page.contains("0 of 0"));
    assert!(page.contains("No network traffic yet") && !page.contains("data-node="));
    assert!(
        page.contains("id=\"graph\">"),
        "the graph sits on the panel, as in the design"
    );
    for column in [">Time<", ">Sandbox<", ">Destination<", ">Verdict<"] {
        assert!(page.contains(column), "{column}");
    }
    assert!(!page.contains("id=\"denied-hosts\""));
}

#[tokio::test]
async fn the_split_view_has_an_accessible_separator_and_the_details_panel_is_a_dialog() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let page = text(app.get("/egress/log", &cookie).await).await;
    let separator = &page[page.find("role=\"separator\"").unwrap()..];
    let separator = &separator[..separator.find('>').unwrap()];
    for attribute in [
        "aria-valuenow=\"520\"",
        "aria-valuemin=\"398\"",
        "aria-valuemax=",
        "tabindex=\"0\"",
        "aria-orientation=\"vertical\"",
    ] {
        assert!(separator.contains(attribute), "{attribute} in {separator}");
    }
    assert!(
        separator.contains("divKey($event)") && separator.contains("setW(520)"),
        "{separator}"
    );
    assert!(
        page.contains("grid-template-columns:var(--graph-w,520px) 16px minmax(0,1fr)"),
        "{page}"
    );
    assert!(
        page.contains("df.traffic.graph-w") && page.contains("try {"),
        "stored in try/catch"
    );
    // The markup does not depend on a stored value: the width is applied by Alpine.
    // Object form: Alpine sets the property and keeps the static grid style.
    assert!(
        page.contains(":style=\"{'--graph-w': w + 'px'}\""),
        "{page}"
    );
    assert!(
        page.contains("x-on:resize.window=\"setW(want, false)\""),
        "re-clamped on resize"
    );
    assert!(page.contains("role=\"dialog\"") && page.contains("aria-labelledby=\"flow-title\""));
    assert!(page.contains("Requests inside TLS tunnels are not inspected."));
    assert!(page.contains("button.closest('tr')"));
    assert!(page.contains("htmx:after:swap") && page.contains("keydown.escape.window"));
    // Pause removes the connect element and Resume inserts it again.
    assert!(
        page.contains("id=\"sse-slot\"")
            && page.contains("slot.replaceChildren()")
            && page.contains("htmx.process")
    );
}

#[tokio::test]
async fn pause_stops_the_stream_explicitly_and_the_stored_width_is_clamped() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let page = text(app.get("/egress/log", &cookie).await).await;
    // Nothing observes the removal of the connect element: the request is aborted by hand, which
    // also ends a reconnect that has no reader yet. Cancelling the reader of an aborted stream
    // rejects with the abort error: swallow it, or it is logged as unhandled.
    let pause = &page[page.find("togglePause() {").unwrap()..];
    let abort = pause.find("abortController.abort()").unwrap();
    assert!(
        abort < pause.find("slot.replaceChildren()").unwrap(),
        "{pause}"
    );
    assert!(pause.contains("sse.reader.cancel().catch(() =&gt; undefined)"));
    // The stored width is passed through the limits once the layout exists.
    assert!(page.contains("this.$nextTick(() =&gt; this.setW(this.want, false))"));
    // Only a drag, a key or a double click remembers a width; fitting does not.
    assert!(page.contains("setW(value, persist = true)"));
    // Clamp first, then remember what is shown.
    assert!(page.contains("Math.round(persist ? value : this.want)"));
    assert!(page.contains("if (persist) {"));
    assert!(page.contains("this.want = this.w"));
    assert_eq!(page.matches("localStorage.setItem").count(), 1);
}

#[tokio::test]
async fn the_advanced_badge_follows_the_inputs_and_not_only_the_page_load() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let page = text(app.get("/egress/log?persona=p-1&task=DF-1", &cookie).await).await;
    assert!(page.contains("adv: 2,"), "{page}");
    assert!(
        page.contains("x-on:input=\"sync($el)\"") && page.contains("x-on:change=\"sync($el)\""),
        "the persona select only fires change"
    );
    assert!(page.contains("this.adv = this.advancedCount(form)"));
    assert!(page.contains("x-show=\"adv &gt; 0\""));
    assert!(
        page.contains("['persona', 'task'].filter("),
        "one list for both"
    );
    assert!(page.contains("x-text=\"adv\">2</span>"), "{page}");
    let none = text(app.get("/egress/log", &cookie).await).await;
    assert!(
        none.contains("adv: 0,") && none.contains("x-cloak"),
        "hidden until Alpine shows it"
    );
}

#[test]
fn the_host_search_ignores_case() {
    for (query, host) in [("GitHub", "api.github.com"), ("github", "API.GitHub.com")] {
        let filter = Filter {
            q: query.into(),
            ..Default::default()
        };
        let body = live(filter, &[allowed(host, "DF", 1)]).unwrap();
        assert!(part(&body, "#flows tbody").is_some(), "{query} vs {host}");
    }
}

#[tokio::test]
async fn without_gateway_discovery_the_log_is_unavailable() {
    let app = TestApp::new().await;
    let cookie = app.login().await;
    let page = app.get("/egress/log", &cookie).await;
    assert_eq!(page.status(), StatusCode::SERVICE_UNAVAILABLE);
    let events = app.get("/egress/log/events", &cookie).await;
    assert_eq!(events.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn the_page_as_served_shows_the_backlog_the_stream_sends_first() {
    let events = vec![
        denied("statsig.anthropic.com", "DF", 1_700_000_001),
        allowed("api.anthropic.com", "DF", 1_700_000_002),
        denied("statsig.anthropic.com", "DF", 1_700_000_003),
    ];
    let app = app_with_egress(events).await;
    let cookie = app.login().await;
    let page = text(app.get("/egress/log", &cookie).await).await;
    let frame = SseReader::new(app.get("/egress/log/events", &cookie).await)
        .next()
        .await;

    let rows = part(&frame.data, "#flows tbody").unwrap();
    assert_eq!(rows.matches("<tr").count(), 3, "{rows}");
    assert_eq!(
        inner(&page, "<tbody data-limit", "</tbody>").expect("element in page"),
        rows
    );
    assert!(page.contains(part(&frame.data, "#graph").unwrap()));
    assert_eq!(
        inner(&page, "<span id=\"event-count\"", "</span>").expect("element in page"),
        part(&frame.data, "#event-count").unwrap()
    );
    for (id, target) in [
        ("count-all", "#count-all"),
        ("count-blocked", "#count-blocked"),
        ("count-allowed", "#count-allowed"),
    ] {
        assert_eq!(
            inner(&page, &format!("<span id=\"{id}\""), "</span>").expect("element in page"),
            part(&frame.data, target).unwrap(),
            "{id}"
        );
    }
    assert_eq!(
        inner(&page, "<div id=\"flows-empty\"", "</div>").expect("element in page"),
        ""
    );
    assert!(!page.contains("No network traffic yet"));
}

#[tokio::test]
async fn a_page_without_events_says_there_is_no_traffic() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let page = text(app.get("/egress/log", &cookie).await).await;
    assert!(page.contains("No network traffic yet"), "{page}");
}

#[tokio::test]
async fn the_page_applies_its_filters_to_the_rows_but_not_to_the_graph() {
    let events = vec![denied("bad.com", "DF", 1), allowed("good.com", "WEB", 2)];
    let app = app_with_egress(events).await;
    let cookie = app.login().await;
    let page = text(app.get("/egress/log?task=DF-1", &cookie).await).await;
    let rows = inner(&page, "<tbody data-limit", "</tbody>").expect("element in page");
    assert!(
        rows.contains("bad.com") && !rows.contains("good.com"),
        "{rows}"
    );
    assert!(page.contains(">good.com<"));
}
