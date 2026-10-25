# Design

## Context

See `proposal.md`. Current state:

- `runs.number` is per task, starting at 1 (`runs_task_number_unique`, `CHECK (number >= 1)`). The Sandbox CR is named `run-<task key>-<number>` (`crates/df-k8s/src/render.rs:116`). The task page and the transcript show `#<number>`.
- `df_db::active_runs` gives the control plane the active runs: run id, pod IP, task key, project and persona. `crates/df-gateway/src/control/snapshot.rs` turns each into a `SandboxEntry`.
- The gateway matches a connection's source IP against the registry and copies run id, task, project and persona into the `FlowEvent` (`crates/df-gateway/src/proxy.rs:605`).
- `df-web` labels a sandbox with `sandbox_label`: the first 8 characters of the run id, or the source IP (`crates/df-web/src/egress/log/rows.rs:80`). `sandbox_key` (run id, or source IP) keys graph nodes, edges and rows.

## Goals / Non-Goals

**Goals:**
- Name a sandbox on the traffic page the way the rest of the UI and the cluster name it.

**Non-Goals:**
- Linking the sandbox to the task page or to the run in the transcript. That is easy to add once the number is there, but it is a separate decision.
- Showing the Sandbox CR name. The task key and number identify it, and nothing else in the UI shows CR names.
- Replacing the run id as the key. The number is only unique per task, and the id stays the stable key.

## Decisions

### D1. The number travels with the registry, not looked up by the web UI

- `SandboxEntry` and `FlowEvent` get `uint32 run_number`, a new field number in each. `active_runs` selects `r.number`.
- The snapshot builder copies it into the entry, and `Flow::start` copies it into the event, next to `task_key`.

*Alternative:* the web UI looks the number up by run id. Rejected: that is a database query per stream, or a cache, for a value the control plane already sends with the run. The event would also stop being self-contained, and the event is what the details panel and a future export show.

### D2. One label function with a fallback

`sandbox_label(event)` becomes:

- `DF-7 #2` when the event has a task key and a run number;
- the first 8 characters of the run id when the number is 0 (an event from a gateway that predates this change, during a rollout);
- the source IP for an unknown source.

The graph node shows the two parts on its two lines: `#2` in the mono first line, `DF-7` in the muted second line. The table and the details panel show the combined label. `sandbox_key` is unchanged, so edges, dimming and row ids keep the run id.

### D3. Protobuf compatibility

The fields are additive. Old readers ignore them, and new readers see 0 from old writers, which D2 handles. Control plane and gateways can therefore be rolled out in any order.

## Risks / Trade-offs

- [A run number repeats across tasks] → It is always shown with its task key, and the key stays the run id.
- [`.sqlx/` metadata] → `active_runs` changes, so the metadata is regenerated in the same change; plain `cargo` builds check it.

## Migration Plan

None. There is no schema change, since `runs.number` exists, and the protobuf fields are additive. Archive after `network-management-ui-polish`, whose egress-log requirements this change modifies.
