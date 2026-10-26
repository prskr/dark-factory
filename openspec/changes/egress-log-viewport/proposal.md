## Why

On the network traffic page the whole page scrolls: with many events the graph scrolls out of view while reading the table, and the filters disappear with it. Separately, the gateway buffers are bounded by count only, so on a quiet instance the page shows connections from many hours or days ago, which is noise for a live view.

## What Changes

- The page header and the filter bar stay fixed; the split view fills the rest of the viewport height.
- The connections panel and the events panel scroll independently; the table header stays visible while its rows scroll. A graph taller than its panel scrolls inside the panel, and it can also be moved by dragging.
- Flow events older than a maximum age (`gateway.bufferMaxAgeSeconds`, default 3600) are dropped from the gateway buffers and from the control plane's merged buffer, in addition to the count limit.
- Zoom of the graph is not part of this change.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `egress-log`: ring buffer gets a maximum age; page layout with fixed header and filter bar and independently scrolling panels.

## Impact

- `crates/df-web/src/egress/log/page.rs` (layout, drag-to-move in the Alpine state), `graph.rs` (scroll container only), tests.
- `crates/df-gateway/src/flow.rs` (`FlowLog`), `crates/df-gateway/src/control/relay.rs` (`Merged`), `gateway.rs` settings.
- `crates/df-core/src/config.rs` (`GatewayConfig.buffer_max_age_seconds`), `crates/dark-factory/src/main.rs`.
- Helm: `gateway.bufferMaxAgeSeconds` in values, schema, configmap, chart tests and README; `crates/df-gateway/README.md`.
- Design: PenPot board on the "Network" page for the scrolled layout; `docs/design-workflow.md` mapping.
