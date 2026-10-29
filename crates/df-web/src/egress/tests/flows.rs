//! Open and closed rows, backlog, durations and the row limit.

use df_proto::gateway::v1::{FlowEvent, Phase};

use super::{
    allowed, app_with_egress, component_script, denied, filter_from_query, flow, live, new_viewer,
    part,
};
use crate::egress::log::{Filter, VerdictFilter};
use crate::testkit::{SseReader, text};

fn opened(host: &str, project: &str, second: i64) -> FlowEvent {
    FlowEvent {
        phase: Phase::Opened.into(),
        ..allowed(host, project, second)
    }
}

#[test]
fn opened_events_are_open_rows_and_count_as_allowed() {
    let batch = [
        opened("api.anthropic.com", "DF", 1),
        denied("statsig.anthropic.com", "DF", 2),
    ];
    let body = live(Filter::default(), &batch).unwrap();
    let rows = part(&body, "#flows tbody").unwrap();
    assert_eq!(rows.matches("data-outcome=\"open\"").count(), 1, "{rows}");
    assert!(rows.contains(">Open<"));
    // Scenario "Open connections": an open tunnel is allowed for the segments and the graph.
    assert_eq!(part(&body, "#count-allowed"), Some("1"));
    assert_eq!(part(&body, "#count-blocked"), Some("1"));
    let graph = part(&body, "#graph").unwrap();
    assert_eq!(graph.matches("data-blocked=\"true\"").count(), 1);
    let allowed_only = live(
        Filter {
            verdict: Some(VerdictFilter::Allowed),
            ..Default::default()
        },
        &batch,
    )
    .unwrap();
    assert!(
        part(&allowed_only, "#flows tbody")
            .unwrap()
            .contains("api.anthropic.com")
    );
}

#[test]
fn the_marker_is_never_a_row() {
    let marker = FlowEvent {
        backlog_end: true,
        ..Default::default()
    };
    let mut viewer = new_viewer(Filter::default());
    viewer.backlog_message(&[]);
    assert!(viewer.live_message(&[marker]).is_none());
}

#[test]
fn a_replacing_message_is_sent_even_without_events() {
    let body = new_viewer(Filter::default()).backlog_message(&[]);
    assert!(
        body.contains("hx-target=\"#flows tbody\"") && body.contains("innerHTML"),
        "{body}"
    );
    // Graph and counts too, so that an earlier connection's state is cleared.
    assert!(
        !body.contains("afterbegin") && part(&body, "#graph").is_some(),
        "{body}"
    );
    assert_eq!(part(&body, "#event-count"), Some("0 of 0"));
    assert!(
        part(&body, "#flows-empty")
            .unwrap()
            .contains("No network traffic yet")
    );
}

fn closed_of(host: &str, id: u64, second: i64) -> FlowEvent {
    flow(
        FlowEvent {
            phase: Phase::Closed.into(),
            bytes_out: 7,
            duration: Some(df_proto::pbjson_types::Duration {
                seconds: 2,
                nanos: 500_000_000,
            }),
            ..allowed(host, "DF", second)
        },
        id,
    )
}

fn row_id_of(pod: &str, id: u64) -> String {
    format!("flow-{}-{id}", hex::encode(pod))
}

#[test]
fn a_closing_event_replaces_the_open_row_of_its_flow() {
    let mut viewer = new_viewer(Filter::default());
    let mut opened = flow(opened("api.anthropic.com", "DF", 1), 5);
    // Whatever the opening event carries, an open row shows no duration.
    opened.duration = Some(df_proto::pbjson_types::Duration {
        seconds: 1,
        nanos: 0,
    });
    let first = viewer.live_message(&[opened]).unwrap();
    let id = row_id_of("gw-0", 5);
    assert!(
        first.contains(&format!("id=\"{id}\"")) && first.contains("data-outcome=\"open\""),
        "{first}"
    );
    assert!(first.contains("afterbegin"));

    let second = viewer
        .live_message(&[closed_of("api.anthropic.com", 5, 9)])
        .unwrap();
    assert!(second.contains(&format!("hx-target=\"#{id}\"")), "{second}");
    assert!(
        second.contains("outerMorph") && !second.contains("afterbegin"),
        "{second}"
    );
    assert!(
        !second.contains("data-outcome=\"open\"") && second.contains("data-outcome=\"allowed\""),
        "{second}"
    );
    assert!(
        second.contains("2.5 s"),
        "the closed event carries the duration for the panel: {second}"
    );
    assert!(
        !first.contains("1.0 s") && first.contains("&quot;duration&quot;:&quot;&quot;"),
        "an open row has none: {first}"
    );
    // The closing event replaces the opened state of the edge: the graph is not sent again, nor
    // are the counts (the flow was counted once).
    assert!(
        part(&second, "#graph").is_none() && part(&second, "#event-count").is_none(),
        "{second}"
    );

    // The row is no longer open: a repeated closing event is a new row.
    let again = viewer
        .live_message(&[closed_of("api.anthropic.com", 5, 10)])
        .unwrap();
    assert!(again.contains("afterbegin"));
}

#[test]
fn a_closing_event_without_a_known_open_row_is_a_new_row() {
    let mut viewer = new_viewer(Filter::default());
    let message = viewer
        .live_message(&[closed_of("api.anthropic.com", 8, 3)])
        .unwrap();
    assert!(
        message.contains("afterbegin") && !message.contains("outerMorph"),
        "{message}"
    );
    assert!(message.contains(&format!("id=\"{}\"", row_id_of("gw-0", 8))));

    // Opened events filtered out of the view leave nothing to replace either.
    let mut denied_only = new_viewer(Filter {
        denied: "1".into(),
        ..Default::default()
    });
    let hidden = denied_only
        .live_message(&[flow(opened("a.com", "DF", 1), 1)])
        .unwrap();
    assert!(
        part(&hidden, "#flows tbody").is_none(),
        "no row, only counts and graph"
    );
    let denied_close = flow(denied("a.com", "DF", 2), 1);
    let message = denied_only.live_message(&[denied_close]).unwrap();
    assert!(message.contains("afterbegin"), "{message}");
}

#[test]
fn the_backlog_has_one_row_per_flow_and_tracks_the_open_ones() {
    let mut viewer = new_viewer(Filter::default());
    let backlog = [
        flow(opened("done.example.com", "DF", 1), 1),
        flow(opened("still-open.example.com", "DF", 2), 2),
        closed_of("done.example.com", 1, 3),
        flow(denied("evil.example.com", "DF", 4), 3),
    ];
    let body = viewer.backlog_message(&backlog);
    assert!(
        body.contains("innerHTML") && !body.contains("afterbegin"),
        "{body}"
    );
    let rows = part(&body, "#flows tbody").unwrap();
    assert_eq!(rows.matches("<tr ").count(), 3, "{rows}");
    assert_eq!(rows.matches("data-outcome").count(), 3, "{rows}");
    assert_eq!(rows.matches("data-outcome=\"open\"").count(), 1, "{body}");
    assert!(rows.contains("still-open.example.com") && rows.contains("evil.example.com"));
    // Three flows, counted once each although two have an opening and a closing event.
    assert_eq!(part(&body, "#event-count"), Some("3 of 3"));

    // The flow that is still open is replaced when it closes; the collapsed one is not tracked.
    let close_open = viewer
        .live_message(&[closed_of("still-open.example.com", 2, 9)])
        .unwrap();
    assert!(close_open.contains(&format!("hx-target=\"#{}\"", row_id_of("gw-0", 2))));
    let late = viewer
        .live_message(&[closed_of("done.example.com", 1, 10)])
        .unwrap();
    assert!(late.contains("afterbegin"), "{late}");
}

#[test]
fn a_rollout_does_not_make_hosts_blocked() {
    let shutdown = FlowEvent {
        reason: "gateway-shutdown".into(),
        ..closed_of("api.anthropic.com", 4, 2)
    };
    let batch = [denied("statsig.anthropic.com", "DF", 1), shutdown];
    let default = Filter::default();
    for body in [
        live(default.clone(), &batch).unwrap(),
        new_viewer(default).backlog_message(&batch),
    ] {
        let graph = part(&body, "#graph").unwrap();
        assert_eq!(graph.matches("data-blocked=\"true\"").count(), 1, "{graph}");
        let blocked = &graph[graph.find("data-blocked=\"true\"").unwrap()..];
        assert!(
            blocked[..blocked.find("</div>").unwrap()].contains("statsig"),
            "{graph}"
        );
        assert_eq!(part(&body, "#count-blocked"), Some("1"));
        // The shutdown still has its row.
        assert!(
            part(&body, "#flows tbody")
                .unwrap()
                .contains("gateway-shutdown")
        );
    }
}

#[tokio::test]
async fn every_connection_starts_by_replacing_the_rows_with_the_backlog() {
    let app = app_with_egress(vec![denied("statsig.anthropic.com", "DF", 1_700_000_001)]).await;
    let cookie = app.login().await;
    // The reconnect after a token refresh replays the same backlog: it must replace, not prepend.
    for _ in 0..2 {
        let response = app.get("/egress/log/events", &cookie).await;
        let frame = SseReader::new(response).next().await;
        assert!(frame.data.contains("innerHTML"), "{}", frame.data);
        assert!(!frame.data.contains("afterbegin"), "{}", frame.data);
        assert!(frame.data.contains("statsig.anthropic.com"));
    }
    let empty = app_with_egress(vec![]).await;
    let cookie = empty.login().await;
    let frame = SseReader::new(empty.get("/egress/log/events", &cookie).await)
        .next()
        .await;
    assert!(
        frame.data.contains("innerHTML") && !frame.data.contains("<tr"),
        "{}",
        frame.data
    );
}

fn message_of(event: &FlowEvent) -> String {
    live(Filter::default(), std::slice::from_ref(event)).unwrap()
}

/// The `duration` the details panel gets from a row of `message`.
fn duration_of(message: &str) -> String {
    let key = "&quot;duration&quot;:&quot;";
    let rest = &message[message.find(key).unwrap() + key.len()..];
    rest[..rest.find("&quot;").unwrap()].to_string()
}

#[test]
fn durations_are_short_and_human() {
    let at = |seconds, nanos| {
        let mut event = closed_of("a.com", 1, 1);
        event.duration = Some(df_proto::pbjson_types::Duration { seconds, nanos });
        duration_of(&message_of(&event))
    };
    assert_eq!(at(0, 12_000_000), "12 ms");
    assert_eq!(at(3, 400_000_000), "3.4 s");
    assert_eq!(at(125, 0), "2:05");
}

#[test]
fn opened_and_closed_in_one_batch_make_one_closed_row() {
    let mut viewer = new_viewer(Filter::default());
    let batch = [
        flow(opened("a.example.com", "DF", 1), 9),
        closed_of("a.example.com", 9, 2),
    ];
    let body = viewer.live_message(&batch).unwrap();
    let id = row_id_of("gw-0", 9);
    assert_eq!(body.matches(&format!("id=\"{id}\"")).count(), 1, "{body}");
    assert!(
        !body.contains("outerMorph") && !body.contains("data-outcome=\"open\""),
        "{body}"
    );
    // Nothing stays tracked: a later close of that flow is a new row.
    let later = viewer
        .live_message(&[closed_of("a.example.com", 9, 3)])
        .unwrap();
    assert!(later.contains("afterbegin"));
}

#[test]
fn a_viewer_stops_tracking_open_rows_past_the_cap() {
    let mut viewer = new_viewer(Filter::default());
    let cap = crate::egress::log::MAX_OPEN_ROWS as u64;
    let batch: Vec<_> = (1..=cap + 1)
        .map(|id| flow(opened("a.example.com", "DF", 1), id))
        .collect();
    viewer.live_message(&batch).unwrap();
    // The early flows were dropped from the set: their close is a new row. The latest is kept.
    let early = viewer
        .live_message(&[closed_of("a.example.com", 1, 5)])
        .unwrap();
    assert!(
        early.contains("afterbegin") && !early.contains("outerMorph"),
        "{early}"
    );
    let latest = viewer
        .live_message(&[closed_of("a.example.com", cap + 1, 5)])
        .unwrap();
    assert!(latest.contains("outerMorph"), "{latest}");
}

#[test]
fn a_rollout_is_not_shown_by_the_blocked_filter() {
    let shutdown = FlowEvent {
        reason: "gateway-shutdown".into(),
        ..closed_of("api.anthropic.com", 4, 2)
    };
    let blocked_only = Filter {
        denied: "1".into(),
        ..Default::default()
    };
    let message = live(blocked_only.clone(), std::slice::from_ref(&shutdown)).unwrap();
    assert!(part(&message, "#flows tbody").is_none(), "{message}");
    let backlog = new_viewer(blocked_only).backlog_message(&[shutdown]);
    assert!(
        !part(&backlog, "#flows tbody")
            .unwrap()
            .contains("api.anthropic.com"),
        "{backlog}"
    );
    assert_eq!(part(&backlog, "#event-count"), Some("0 of 1"));
}

#[test]
fn absurd_durations_neither_overflow_nor_go_negative() {
    let at = |seconds, nanos| {
        let mut event = closed_of("a.com", 1, 1);
        event.duration = Some(df_proto::pbjson_types::Duration { seconds, nanos });
        duration_of(&message_of(&event))
    };
    assert!(at(i64::MAX, 999_999_999).contains(':'));
    assert_eq!(at(-5, 0), "0 ms", "negative values clamp to zero");
    assert_eq!(at(0, -7_000_000), "0 ms");
}

#[test]
fn the_row_limit_is_a_view_setting_with_fixed_choices() {
    assert_eq!(Filter::default().row_limit(), 500);
    let rows = filter_from_query("rows=100");
    assert_eq!(rows.row_limit(), 100);
    assert_eq!(rows.query(), "?rows=100", "kept in the URL and the stream");
    assert_eq!(rows.advanced_count(), 0);
    assert!(rows.is_empty(), "no filter: Clear filters stays hidden");
    // The default is not written; other values fall back to it.
    assert_eq!(filter_from_query("rows=500").query(), "");
    for odd in ["rows=7", "rows=all", "rows=-1"] {
        assert_eq!(filter_from_query(odd).row_limit(), 500, "{odd}");
    }
}

#[test]
fn the_backlog_sends_only_the_newest_rows_up_to_the_limit() {
    let backlog: Vec<_> = (1..=150)
        .map(|i| {
            flow(
                allowed(&format!("h{i}.com"), "DF", i),
                u64::try_from(i).unwrap(),
            )
        })
        .collect();
    let body = new_viewer(filter_from_query("rows=100")).backlog_message(&backlog);
    let rows = part(&body, "#flows tbody").unwrap();
    assert_eq!(rows.matches("data-outcome=").count(), 100, "{rows}");
    assert!(rows.contains("h150.com:443") && rows.contains("h51.com:443"));
    assert!(!rows.contains("h50.com:443"));
    // The counts still cover every event.
    assert_eq!(part(&body, "#event-count"), Some("150 of 150"));
}

#[test]
fn a_flow_whose_row_was_trimmed_closes_as_a_new_row() {
    // Flow 1 is open and the oldest of 100 rows: the next new row pushes it out of the table.
    let mut backlog = vec![flow(opened("a.example.com", "DF", 1), 1)];
    backlog.extend((2..=100).map(|i| {
        flow(
            allowed(&format!("h{i}.com"), "DF", i),
            u64::try_from(i).unwrap(),
        )
    }));
    let mut viewer = new_viewer(filter_from_query("rows=100"));
    let body = viewer.backlog_message(&backlog);
    assert_eq!(
        part(&body, "#flows tbody")
            .unwrap()
            .matches("data-outcome=")
            .count(),
        100
    );
    viewer
        .live_message(&[flow(allowed("new.example.com", "DF", 200), 200)])
        .unwrap();
    // The page no longer has its row: the close is a new row, not a replacement.
    let close = viewer
        .live_message(&[closed_of("a.example.com", 1, 201)])
        .unwrap();
    assert!(
        close.contains("afterbegin") && !close.contains("outerMorph"),
        "{close}"
    );
    // A flow still in the table is replaced in place.
    let mut viewer = new_viewer(filter_from_query("rows=100"));
    viewer.backlog_message(&backlog);
    let close = viewer
        .live_message(&[closed_of("a.example.com", 1, 201)])
        .unwrap();
    assert!(close.contains("outerMorph"), "{close}");
}

#[tokio::test]
async fn the_rows_choice_sits_in_the_filter_bar_and_the_page_trims_the_table() {
    let app = app_with_egress(vec![]).await;
    let cookie = app.login().await;
    let page = text(app.get("/egress/log?rows=1000", &cookie).await).await;
    let select = &page[page.find("name=\"rows\"").unwrap()..];
    let select = &select[..select.find("</select>").unwrap()];
    for value in ["100", "250", "500", "2500", "5000"] {
        assert!(
            select.contains(&format!("<option value=\"{value}\">")),
            "{value}: {select}"
        );
    }
    assert!(
        select.contains("<option value=\"1000\" selected>"),
        "{select}"
    );
    assert!(
        page.find("name=\"rows\"").unwrap() < page.find("id=\"traffic\"").unwrap(),
        "part of the filter form"
    );
    assert!(page.contains("<tbody data-limit=\"1000\">"), "{page}");
    // Live rows past the limit are dropped from the bottom after every swap.
    assert!(component_script().contains("tbody.rows.length > limit"));
    // Clear filters resets the persona choice, not the row limit.
    assert!(component_script().contains("input[type=text], select[name=persona]')"));
    let default = text(app.get("/egress/log", &cookie).await).await;
    assert!(
        default.contains("<option value=\"500\" selected>")
            && default.contains("data-limit=\"500\"")
    );
}
