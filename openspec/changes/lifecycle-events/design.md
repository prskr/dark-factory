# Design

## Context

See proposal.md (Why) and [ADR-0049](../../../docs/adr/0049-lifecycle-events-outbox-and-ed25519-signatures.md). The current state this design builds on:

- **Task volume:** one per task (`task-<key>`). The scheduler calls `SandboxProvider::ensure_task_volume` before *every* run (`crates/df-agent-channel/src/scheduler.rs`). It is an idempotent server-side apply that cannot tell "created" from "already there". Volume deletion is planned in `iter2-task-volume-lifecycle` and not implemented yet.
- **Run states:** transitions go through `set_run_state_tx`, `mark_started` (`Connecting`, pod up), `mark_connected` and `end_run_tx` / `finish_run` (teardown) in `crates/df-agent-channel/src/runs.rs`. Provisioning errors go through the scheduler's `provisioning_failed` path.
- **Run times:** `runs.started_at` is the run's creation time, not the time its sandbox came up. No column records when provisioning began or when the pod started.
- **Connect timeout:** `runs.connectTimeoutSeconds` (default 600 s) counts from run creation across `Pending`, `Provisioning` and `Connecting`. A pod waiting for a node that is still booting stays `Pending` within this budget.
- **Notifications:** LISTEN/NOTIFY (`crates/df-db/src/notify.rs`) fans out ID-only notifications without delivery guarantees. There is no outbox.
- **Leader:** the control plane already elects a leader via a Lease (`crates/dark-factory/src/sandbox.rs`, `is_leader` watch channel).
- **Dependencies:** `reqwest` (rustls) and `ring` are already workspace dependencies. No control-plane crate makes outgoing HTTP calls yet.
- **Roles:** only `board-member` exists (`crates/df-web/src/auth/mod.rs`, `REQUIRED_ROLE`).

## Goals / Non-Goals

**Goals:**
- Events are recorded transactionally with the state change and delivered by one dispatcher on the leader.
- A slow or hostile receiver cannot affect runs, other receivers, or reach internal services, for project subscriptions.
- Adding an event type later means one enqueue call and one catalog entry.
- Another component can use the outbox by registering a handler for a new `kind` (first planned: `pr.rs`, TD-91).

**Non-Goals:**
- Hot reload of the signing key or instance subscriptions. Both are read at startup, and a change needs a control plane restart. Backlog: "Lifecycle event extensions".
- Per-receiver rate limiting or concurrency settings. Rejected for now, because the global in-flight cap (D3) is enough for the expected handful of receivers.

## Decisions

### D1: Generic outbox, lifecycle events as one kind

There are two layers.

**Generic outbox (`df-db::outbox`)**, one table `outbox`:
- `id`, `kind TEXT`, `payload JSONB`;
- `attempts`, `next_attempt_at`, `lease_until`;
- `done_at`, `gave_up_at`, `last_error`;
- `created_at`.

`outbox::enqueue(tx, kind, payload)` runs inside the caller's transaction and notifies `outbox` on commit.

Nothing in the table knows about HTTP or events. A consumer is a handler for one `kind`:

```rust
/// Delivers one outbox job of a single kind.
trait OutboxHandler: Send + Sync {
    fn kind(&self) -> &'static str;
    fn retry_policy(&self) -> RetryPolicy; // backoff base, cap, give-up age
    async fn handle(&self, payload: &serde_json::Value) -> Result<(), JobError>; // JobError::Retry | JobError::GiveUp
}
```

**Lifecycle events on top:**
- **`lifecycle_events`:** `id UUID` (v7), `type`, `occurred_at`, `project_id`, `payload JSONB`. This is the full envelope, built once.
- **`lifecycle_events::enqueue(tx, event, instance_subscriptions)`** inserts the event and one outbox job of kind `lifecycle_event.deliver` per matching target. The job payload is `{event_id, target}`, where `target` is `{subscription_id}` or `{instance_url}`. With no match it writes nothing.
- **The handler** loads the event, resolves the target, signs and POSTs it, and records `last_attempt_at`, `last_status` and `last_error` on the project subscription row.

The payload is a snapshot. A persona renamed later doesn't change events already recorded.

**Why a generic table:** the lease, retry and cleanup logic is the subtle part, and the next consumers would otherwise copy it. The first planned one is the forge calls in `pr.rs` (TD-91). Keeping one row per (event, target) means a dead receiver never holds back another.

Alternative: a delivery table specific to lifecycle events. It was rejected because the second consumer would duplicate the dispatcher.

### D2: Trigger points

| Event | Transaction |
|---|---|
| `volume.provisioned` | After `ensure_task_volume` succeeds, the scheduler runs `UPDATE tasks SET volume_provisioned_at = now() WHERE id = $1 AND volume_provisioned_at IS NULL RETURNING …`. Only when a row comes back does it enqueue the event, carrying the run being scheduled. |
| `sandbox.provisioning` | The scheduler's transition to `Provisioning`, before `ensure_task_volume`. Also sets the new `runs.provisioning_started_at`. |
| `sandbox.provisioned` | `mark_started`. Also sets the new `runs.sandbox_started_at`. |
| `sandbox.provisioning_failed` | The `provisioning_failed` state change. |
| `sandbox.destroyed` | `end_run_tx`, if `provisioning_started_at IS NOT NULL` and the run did not end through `provisioning_failed`. Carries `outcome`, `started_at = sandbox_started_at` (`null` when the pod never came up) and `ended_at`. |
| `volume.destroyed` | The volume deletion transaction of `iter2-task-volume-lifecycle` (`volume_deleted_at` set): reason `retention` from the retention scan, `reset` from reset workspace. The same transaction resets `volume_provisioned_at` to `NULL`, so a later recreation (for example after a reset) emits `volume.provisioned` again. |

**Pairing:** every `sandbox.provisioning` is followed by exactly one `sandbox.provisioning_failed` or `sandbox.destroyed` for the same run. Both closing events are written in transactions that end the run, and a run ends only once, so a run can't get two closing events. A run cancelled while still `Provisioning` gets `sandbox.destroyed` with `started_at: null`. Delivery order is not guaranteed, so receivers key on `run.id`.

**Why `sandbox.provisioning` and not an event at run creation:** a run can wait in the queue (instance concurrency limit, `iter2-run-resilience`), so an event at creation could wake compute too early. The moment the scheduler picks the run is the moment compute is needed. Dark Factory doesn't wait for receivers. A node that boots on this event only has to be ready within the connect timeout.

The migration backfills `volume_provisioned_at = now()` for tasks that already have a run, so existing tasks emit no late `volume.provisioned`.

A crash between `ensure_task_volume` and the marker update re-runs both on the next schedule. The event then fires at that point, still exactly once.

### D3: Outbox dispatcher on the leader

A task started when `is_leader` becomes true and stopped when leadership is lost. It holds the registered handlers by `kind`, so a new consumer registers a handler and changes nothing else.

**Main loop:**
1. Claim due jobs with `SELECT … FROM outbox WHERE done_at IS NULL AND gave_up_at IS NULL AND next_attempt_at <= now() AND (lease_until IS NULL OR lease_until < now()) ORDER BY next_attempt_at LIMIT 8 FOR UPDATE SKIP LOCKED`. One claim is one wave, so no claimed job waits for a free slot while its lease runs.
2. Set `lease_until = now() + lease` (the handler's timeout plus a margin), then commit.
3. Run the claimed handlers at once; a job claimed after the policy's give-up age is given up without running (the dispatcher alone decides on the age).
4. Write the result back:
   - `Ok` sets `done_at`;
   - `Retry` sets `next_attempt_at` from the handler's `RetryPolicy`;
   - `GiveUp` sets `gave_up_at`.

A job whose `kind` has no handler is left untouched, with a warning. This happens, for example, during a rolling update that adds a kind.

**Waking up:** LISTEN/NOTIFY `outbox`, with a 5 s poll as fallback.

**Retry policy for `lifecycle_event.deliver`:** attempt *n* fails → `min(30 s · 2^(n-1), 1 h)`, giving up after 24 h.

**Cleanup:** once an hour the dispatcher deletes jobs that finished or were given up more than 7 days ago. The lifecycle handler's cleanup hook deletes `lifecycle_events` rows that no job references any more.

**Why a lease instead of holding the row lock during the handler:** no transaction stays open across a 10 s HTTP call. After a crash, the lease expires and the job runs again. That is at-least-once, and every handler must tolerate it.

### D4: SSRF protection in the resolver

Project subscriptions are sent through a `reqwest::Client` with three properties:
- **A custom DNS resolver** (`reqwest::dns::Resolve`). It resolves the host and refuses it with `address refused` if any address is loopback, private (RFC 1918), CGNAT (`100.64.0.0/10`), link-local, unique-local (`fc00::/7`), unspecified, broadcast or documentation, or an IPv4-mapped IPv6 form of these (spec "Project subscription URL restrictions").
- `redirect::Policy::none()`: a redirect counts as a non-2xx response.
- `no_proxy()`.
- **Literal IP addresses in the URL** are checked when the subscription is saved and again before sending, since IP-literal URLs never go through the resolver.

Instance subscriptions use a second client without the filter. Both clients use a 10 s total timeout.

**Why filter in the resolver:** the address that is checked is the address the client connects to, so there's no window for DNS rebinding.

Alternative: resolve, check, then connect by IP. That breaks TLS SNI and certificate verification unless the request is rebuilt by hand.

Address classification uses `std::net` (`is_private`, `is_loopback`, `is_link_local`, `is_unique_local`, `to_ipv4_mapped`, …), plus one CIDR check for CGNAT.

### D5: Signing

**Loading the key:** at startup the control plane reads `lifecycleEvents.signingKeyFile` (PEM, PKCS#8). cert-manager writes PKCS#8 v1 `PRIVATE KEY`, so the key is loaded with `ring::signature::Ed25519KeyPair::from_pkcs8_maybe_unchecked`.

**Signing:** each attempt signs `"{id}.{timestamp}.{body}"`. The timestamp is the attempt's, so receivers' replay windows still work on retries. The header value is `v1a,{base64(sig)}`.

**Public key:** `whpk_{base64(public_key)}`, computed once at startup.

**Without a key file**, for example in a dev setup that has none:
- the lifecycle event feature is off: `enqueue` writes nothing;
- the key endpoint returns 404;
- the project settings say events are disabled.

With subscriptions configured but no key, the control plane refuses to start. `just serve-local` generates a throwaway key next to the agent certificate.

### D6: Configuration

```yaml
# control plane config.yaml (rendered by the chart)
lifecycleEvents:
  signingKeyFile: /run/secrets/lifecycle-events/tls.key
  subscriptions:
    - url: http://audit.observability.svc:8080/events
      events: [sandbox.destroyed]
```

The chart renders the Certificate (D7) or uses `existingSecret`, then mounts the key read-only and writes the path. The event type list is one enum in `df-core`. The Helm values schema repeats it, and a chart unit test compares the two.

### D7: Helm signing key Certificate

The Certificate has:
- `secretName: <fullname>-lifecycle-events-key`;
- `issuerRef` from the existing `dark-factory.issuerRef` helper;
- `privateKey: {algorithm: Ed25519, rotationPolicy: Never}`;
- `commonName: <fullname>-lifecycle-events` (required by the issuer, otherwise unused);
- the chart's `tls.duration` / `tls.renewBefore`.

**Renewal:** it keeps the key, because of `rotationPolicy: Never`.

**Manual rotation:** delete the Secret and restart the control plane.

Note that cert-manager ≥ 1.18 defaults `rotationPolicy` to `Always`, which is why the chart sets it explicitly.

### D8: `admin` role check

A `require_role(user, "admin")` check runs on the project subscription routes, and the 403 page names the missing role. The routes sit under the existing project settings (`crates/df-web/src/config_pages.rs`).

`dev/dex.yaml` and `deploy/e2e/dex.yaml` add the group `admin` to `admin@example.com`, and the e2e seed covers the admin path.

The generic `can()` permission check from ADR-0011 is not built (proposal, out of scope).

### D9: Project subscription table

`event_subscriptions` has these columns:
- `id`, `project_id` (FK, cascade delete);
- `url`, `events TEXT[]` (empty means all);
- `created_at`;
- `last_attempt_at`, `last_status`, `last_error`.

Deleting a subscription leaves its pending jobs; the handler gives them up when the subscription no longer exists.

## Risks / Trade-offs

- [The outbox grows if receivers are down] → Each handler's give-up age and the 7-day cleanup bound it. For lifecycle events that is about one day of events per subscription.
- [A receiver sees duplicates after a crash mid-request] → The contract is at-least-once, and `webhook-id` is stable for deduplication. The receiver reference says so.
- [A hostile project subscription target hangs connections] → 10 s timeout, at most 8 requests in flight in total, and project subscriptions need `admin`.
- [A shared in-flight cap lets one slow receiver delay others by up to 10 s per batch] → Acceptable for a handful of receivers. Per-subscription concurrency goes on the backlog if it shows up.
- [Key rotation breaks receivers until they re-fetch] → It's documented, and the key endpoint makes re-fetching automatable.
- [The address filter misses an internal range specific to a cluster, e.g. a public pod CIDR] → Operators who need stricter limits restrict control plane egress with a NetworkPolicy. The receiver reference mentions it.

## Migration Plan

Additive. The migration adds the `outbox`, `lifecycle_events` and `event_subscriptions` tables, the `tasks.volume_provisioned_at` column (with backfill) and the `runs.provisioning_started_at` and `runs.sandbox_started_at` columns. Rollback: the control plane of the previous version ignores the new tables and columns.

The chart creates the Certificate on upgrade. Without subscriptions, nothing is sent.
