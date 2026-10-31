# Design

## Context

- The gateway (`iter1-egress-gateway`) is the same binary as the control plane (`dark-factory gateway`), runs at least 2 replicas, and has no database or Kubernetes credentials. It gets everything from `PolicyWatch` and identifies sandboxes by pod IP.
- The runtime always uses `Assign.workspace.repo_url` and never derives URLs (`design/agent-run.md` §11). Switching to the proxy is therefore a control-plane change plus the removal of the credential helper.
- Forge credentials are Kubernetes Secret references resolved by the control plane's `SecretStore` (`iter1-sandbox-provisioning`).
- Push checks (secret scan, identity, protected paths, …) are **not** part of this change. `iter2-push-checks` plugs them into the quarantine step defined here (D5).

## Goals / Non-Goals

**Goals:**
- Ref rules are a pure function `check_refs(route, commands, is_ancestor) -> Vec<RefVerdict>`, unit-tested exhaustively.
- Fetch traffic is streamed in both directions. Only pushes are spooled, because they must be inspected before forwarding.

**Non-Goals:**
- Git LFS, SSH, the dumb HTTP protocol.
- Branch rewrite policies (ADR-0033). The task branch is fast-forward only.
- A UI for push records (the audit log view comes in iteration 4, `product/iterations/roadmap.md`). Records are stored and audited.
- Git traffic in the live egress log. Push records cover pushes; fetches are not logged per request.

## Decisions

### D1 — Contract additions (`darkfactory.gateway.v1`)
```proto
message SandboxEntry { …; GitRoute git = 7; }
message GitRoute {
  string upstream_url = 1;      // project repository, https (http with forge.allowPlainHttp)
  string base_branch = 2; string task_branch = 3; string wip_branch = 4;
  string credential_ref = 5;    // key into Snapshot.forge_credentials
  string commit_name = 6; string commit_email = 7;  // used by iter2-push-checks
}
message Snapshot { …; map<string, ForgeCredential> forge_credentials = 2; }
message ForgeCredential { string username = 1; string password = 2; }   // Debug redacted

service GatewayGit { rpc RecordPush(PushRecord) returns (RecordPushResponse); } // control plane, port 9444
message PushRecord {
  string push_id = 1;           // UUIDv7, idempotency key
  string run_id = 2; string gateway_pod = 3;
  repeated RefUpdate refs = 4;  // name, old_sha, new_sha, verdict, reason
  PushVerdict verdict = 5;      // FORWARDING | ACCEPTED | REJECTED | UPSTREAM_FAILED
  uint64 bytes = 6; google.protobuf.Duration duration = 7;
}
```
The agent contract drops `CredentialRequest`/`CredentialGrant` (pre-MVP, no mixed versions); field numbers 14 (`AgentMessage`) and 16 (`ControlMessage`) are reserved.

*Alternative:* credentials pushed over a separate stream. Rejected because one snapshot keeps the route and its credential consistent with each other.

### D2 — URL and identity
`https://<gateway-svc>.<ns>.svc:8443/git/<KEY>.git`. The host name comes from instance configuration, which the chart sets to the Service DNS name. The proxy looks up the source IP in the snapshot and compares `<KEY>` with the entry's `task_key`. Pod IP instead of mTLS: the harness can read the pod's key anyway, and the egress proxy already relies on the same identification (iteration-2 decision 1).

### D3 — Forwarding
- hyper server with rustls (gateway certificate) on 8443. A hyper client builds upstream requests to `<upstream_url>/info/refs?service=…` and `<upstream_url>/git-upload-pack|git-receive-pack`.
- Forwarded request headers: `Content-Type`, `Content-Encoding`, `Accept`, `Git-Protocol`, `User-Agent`. Everything else, `Authorization` in particular, is dropped. The proxy adds `Authorization: Basic <username:password>`.
- Response bodies are streamed back unchanged. Upstream status codes are passed through, except 401/403, which become 502 with a fixed message, so that the client never sees an authentication challenge for the forge. A connection failure or timeout towards the upstream becomes 503, the same status as "no snapshot yet", so that the runtime can retry exactly the 503 cases and nothing else.
- The upstream must be HTTPS (plain HTTP only with `forge.allowPlainHttp`, for throwaway clusters such as `just e2e-local`; the gateway checks the setting itself), and redirects are not followed.

### D4 — Ref rules
`receive-pack` request bodies start with pkt-line ref commands (`<old> <new> <ref>\0<caps>`), followed by the pack. They are parsed with `gix-packetline`. Rules, in order:
1. `refs/heads/factory/wip/<KEY>`: always allowed.
2. `refs/heads/factory/task/<KEY>`: create allowed; delete rejected; update allowed if `old` is an ancestor of `new` (checked in the mirror plus the quarantined pack).
3. Anything else: `df:ref-not-allowed`.

One rejected command rejects the whole push, so the forge never sees a partially applied push. The response is a synthesized `report-status` (`unpack ok`, then `ng <ref> <reason>` for every command), wrapped in side-band-64k if the client asked for it.

### D5 — Spool, quarantine and mirror
- The push body is spooled to a temp file in the mirror volume. A body larger than `maxPushSize` is rejected while reading.
- **Mirror:** one bare repository per upstream URL (directory name = SHA-256 of the URL), created on first use. Before a push is checked, the mirror fetches `+refs/heads/*:refs/heads/*` from upstream with the forge credential (`refs/*` would also mirror `refs/pull/*` on GitHub; the ref rules only need branches). Fetches of the same mirror are serialized with a per-mirror `tokio::sync::Mutex`. Push spools are not serialized.
  - *ponytail:* full mirror, no partial clone (gitoxide has no promisor support yet), and no eviction beyond the `emptyDir` size limit. Add LRU eviction when mirrors outgrow the volume.
- **Quarantine:** the pack is indexed into a temp object directory with thin-pack bases resolved from the mirror (`gix-pack` bundle writing with a base-object lookup into the mirror's object database). The ancestor check and later push checks read from mirror + quarantine.
- **Check hook:** after the ref rules, a `PushChecks` trait (single method, consumed by the Git endpoint) receives the route, the commands and the quarantine. This change ships a no-op implementation; `iter2-push-checks` provides the real one.
- After an accepted push, the original spooled body is forwarded to upstream `git-receive-pack`, and the upstream response is relayed. The mirror is not updated after the push; the next push's fetch picks the change up.

### D6 — Push records
The control plane's `RecordPush` upserts `git_pushes` by `push_id` (one row per push, with the ref updates as JSONB) and writes an audit entry `git.push` with the agent actor of the run, in one transaction. The verdict only moves forward: `FORWARDING → ACCEPTED | UPSTREAM_FAILED`. `REJECTED`, `ACCEPTED` and `UPSTREAM_FAILED` are final. A call that repeats the stored verdict is a no-op, and a call that would move the verdict backwards is ignored. Retries are therefore idempotent.

1. **Rejected push:** one call with `REJECTED`, then `report-status` to the client.
2. **Push that passed all rules and checks:** first a call with `FORWARDING`. Only after it was stored does the gateway forward the body upstream, so no upstream side effect happens without a durable record. If this call fails after retries with backoff for up to 5 s, the push is rejected with `df:audit-unavailable` and nothing reaches upstream.
3. **After the upstream answered:** a second call with `ACCEPTED` or `UPSTREAM_FAILED`, then the upstream response is relayed. If this call fails, the gateway keeps it in a small in-memory retry queue and still relays the upstream response; the push is not undone.

*ponytail:* if the gateway pod dies before the final call succeeds, the record stays `FORWARDING`. The control plane's timeout scan marks such records older than 10 minutes as `UNKNOWN`. `UNKNOWN` is a database-only placeholder (not in the contract enum): a late `ACCEPTED` or `UPSTREAM_FAILED` from the gateway's retry queue still replaces it and writes its `git.push` audit entry. The record still proves that the push was attempted, and the runtime's `PushReport` shows the result on the run. Make the queue durable if this ever matters.

### D7 — Snapshot building
The snapshot query joins active runs with their project's repository URL and forge credential reference. Credentials are resolved through `SecretStore` while the snapshot is built. Besides notifications, the snapshot is rebuilt every 60 s, so rotated Secrets reach gateways without a Secret watch.

### D8 — Runtime and sandbox wiring
- `factory-agent`: the `git-credential` subcommand, its socket and the `CredentialRequest` client are deleted. The generated `.git/config` sets `credential.helper=` (empty, which resets the list) and `http.<proxy URL>.sslCAInfo=<csi volume>/ca.crt`; the clone gets the same key with `-c`, and `GIT_SSL_CAINFO` is never set (it would override the scoped key). `GIT_TERMINAL_PROMPT=0` stays.
- `df-k8s`: `hostAliases` for the gateway Service names → ClusterIP, those names in `NO_PROXY`, and a per-run policy egress rule to gateway pods on 8443.

## Risks / Trade-offs

- [gitoxide can't resolve thin packs against a separate object database the way D5 needs] → resolved by spike 1.1 (`crates/df-gateway/README.md`, "Git proxy"): `gix-pack` indexes a thin pack with the mirror as base lookup, and the mirror fetch runs over an own hyper-based HTTP backend for `gix-transport`. The `git` CLI fallback is not needed.
- [Mirror fetch before every push adds latency] → incremental fetches of a homelab repository take well under a second; pushes happen once per turn.
- [Gateway in the clone path] → bodies are streamed; clones are blobless.
- [Ancestor check against a stale mirror] → the mirror is fetched right before the check, and upstream `receive-pack` still enforces its own old-value check (lease semantics).

## Migration Plan

1. Deploy the gateway with the Git port and the control plane with `RecordPush` and the snapshot Git routes. The runtime still uses direct URLs, so nothing changes yet.
2. Switch `Assign.workspace.repo_url` to the proxy URL and ship the runtime without the credential helper (same release, since runtime and control plane are deployed together).
3. Remove the forge host from the instance allowlist (documented step; the demo seed no longer adds it).

Rollback: redeploy the previous release. Task branches live on the forge, so existing task volumes keep working with either URL, because the runtime rewrites `.git/config` on every prepare.
