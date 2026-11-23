# Design

## Context

See proposal.md for motivation and the dependency table. Today's `chrono` use, measured on `main` (`9a71335`):

| Crate | What it uses |
|---|---|
| `df-core` | `DateTime<Utc>` in domain types (`domain.rs`, `run_transition.rs`, `task_transition.rs`), `TimeDelta` in `config.rs` (`resume_backoff`) |
| `df-db` | `DateTime<Utc>` in 17 modules; `sqlx` feature `chrono` decodes `timestamptz` (64 columns and parameters in `.sqlx/`); `to_rfc3339_opts(Millis)` for lifecycle event payloads; `pbjson_types::Timestamp` → `DateTime` in `transcript/runtime.rs` |
| `df-orchestrator` | `DateTime<Utc>` and `TimeDelta` in `scheduler`, `retention`, `runs`, `pr/sync`, `testing` (feature `test-util`) |
| `df-agent-channel` | `DateTime<Utc>` in `session`, `assign`; `DateTime<FixedOffset>` and RFC 3339 parsing in `tools/feedback.rs` (`since`, `next_since`); `to_rfc3339()` in the metadata line of `tools/artifacts.rs` |
| `df-gateway-control` | proto timestamp → `DateTime` in `usage_records.rs` |
| `df-web` | `format(…)` and `to_rfc3339()` in `requests.rs`, `artifacts/view.rs`, `chat_pages/view.rs`, `chat_pages/archive.rs`, `components/transcript.rs`, `project_subscriptions.rs`, `egress/log/rows.rs`, `task_pages/view.rs`, `task_pages/resume.rs`; `DateTime<Utc>` in `transcript`; `auth/store.rs` (`web_sessions`, runtime `query_as`, `time::OffsetDateTime` from `tower-sessions`); `testkit.rs` |
| `dark-factory` | `Utc::now()` in `lifecycle_events/handler.rs`, demo data |
| `factory-agent` | `Utc::now().into()` → `pbjson_types::Timestamp` in `harness.rs` and `transcript.rs` (7×); retry time in `runner.rs` |
| `tools/xtask` (standalone workspace) | `Utc::now()` and `Datelike` in `next_version.rs` |

Test code outside `df-db` binds or reads `timestamptz` with runtime `sqlx` queries: `df-agent-channel/src/session/tests.rs`, `df-orchestrator/src/{retention,pr/sync,scheduler}/tests.rs`, `df-orchestrator/src/testing.rs`, `dark-factory/src/lifecycle_events/handler/tests.rs`, `df-web/src/task_pages/tests/resume.rs`, `df-web/src/testkit.rs`.

The database stores only `timestamptz` (no `date`, `time` or `interval` columns). `jiff` 0.2 is compiled today (through `gix-date`, `kube`, `k8s-openapi`); `jiff` 1.0 is in `Cargo.lock` only as an inactive optional dependency of `serde_with`.

## Goals / Non-Goals

**Goals:**
- No workspace crate (and not `tools/xtask`) depends on `chrono` directly.
- Same behavior and same database values. Text formats change only where D4 says so.

**Non-Goals:**
- Clearing `RUSTSEC-2026-0543`: blocked by dependencies. Target: backlog row in `product/iterations/roadmap.md`.
- Other items: see proposal, Out of Scope.

## Decisions

### D1. Type mapping

| `chrono` | `jiff` 1.0 |
|---|---|
| `DateTime<Utc>` | `jiff::Timestamp` |
| `DateTime<FixedOffset>` (only parsed and compared) | `jiff::Timestamp` |
| `TimeDelta`, `chrono::Duration` | `jiff::SignedDuration` |
| `Utc::now()` | `Timestamp::now()` |
| `a - b` (two times) | `a - b` (gives `SignedDuration` in `jiff` 1.0) |
| `t + d`, `t - d`, `checked_*` | the same with `SignedDuration` |
| `TimeDelta::try_seconds(n)`, `TimeDelta::MAX` | `SignedDuration::from_secs(n)`, `SignedDuration::MAX` |
| `Duration::from_std(d)`, `d.to_std()` | `SignedDuration::try_from(d)`, `std::time::Duration::try_from(d)` |
| `t.timestamp()` | `t.as_second()` |
| `Utc.with_ymd_and_hms(…)` (tests) | `"2026-11-23T08:00:00Z".parse::<Timestamp>()` or `civil::date(…).at(…).to_zoned(TimeZone::UTC)?.timestamp()` |
| `format("%b %d %H:%M")` | `t.strftime("%b %d %H:%M")` (`Timestamp` formats in UTC) |
| `to_rfc3339()` | `t.to_string()` (prints `Z`, see D4) |
| `to_rfc3339_opts(Millis, true)` | `format!("{t:.3}")` |
| `DateTime::parse_from_rfc3339(s)` | `s.parse::<Timestamp>()` |
| `Utc::now().year()`, `.month()` (`xtask`) | `Timestamp::now().to_zoned(TimeZone::UTC)` then `.year()`, `.month()` |

`Timestamp` is always UTC, like `DateTime<Utc>`, so no timezone handling enters the code. `jiff` is added with `default-features = false` and features `std`, `serde`. Our crates use no named timezone; `iter3-routines` adds `tzdb-bundle-always` where it needs one (the static musl images have no `/usr/share/zoneinfo`).

As built: `jiff` 1.0 names its zoned type `ZonedDateTime`, so the last row and the test row read `Timestamp::now().to_zoned_datetime(TimeZone::UTC)` (then `.year()`, `.month()`, `.date()`) and `civil::date(…).at(…).to_zoned_datetime(TimeZone::UTC)?.timestamp()`; `to_zoned` does not exist. `Timestamp::checked_add`/`checked_sub` return `Result`, so `chrono`'s `Option` chains end in `.ok()`. `SignedDuration` has `as_secs`/`as_mins` instead of `num_seconds`/`num_minutes`.

Alternative: `time` 0.3, which `sqlx` supports natively and which is already in the graph (`tower-sessions`, `rcgen`). Rejected: `time` has no IANA timezone support, so routines would still need a second library; `jiff` covers both and is what `croner` supports.

### D2. Database: `jiff-sqlx` and a `sqlx.toml` type override

`sqlx` 0.9 has no `jiff` feature. `jiff-sqlx` 1.0 (by the `jiff` author, for `sqlx` 0.9 with `default-features = false`) provides `jiff_sqlx::Timestamp`, a wrapper with `Encode`/`Decode` for `timestamptz` (OID 1184) and `PgHasArrayType` for `timestamptz[]`, plus `ToSqlx`, `to_jiff()` and `From` in both directions. It truncates to microseconds, like `chrono`.

Without the `sqlx` feature `chrono`, the `query!` macros have no Rust type for `timestamptz`. `sqlx` 0.9 reads `sqlx.toml` from the crate's `CARGO_MANIFEST_DIR`, but only with the cargo feature `sqlx-toml` (without it, an existing file is a hard error). `crates/df-db/sqlx.toml`:

```toml
[macros.type-overrides]
# Key is the type's display name and is case-sensitive: 'timestamptz' would be ignored.
'TIMESTAMPTZ' = "jiff_sqlx::Timestamp"
```

The override applies to output columns and bind parameters of every `query!`/`query_as!`/`query_scalar!` in the crate, so it is switched on together with the full `df-db` migration (tasks group 4), never for one query.

How `df-db` code uses it:

- `query_as!` converts each field with `.into()`, and `From<jiff_sqlx::Timestamp> for jiff::Timestamp` exists. Target structs keep `jiff::Timestamp` for NOT NULL columns. Nullable columns (`Option<_>`) have no `From` and use `Option<jiff_sqlx::Timestamp>`, converted with `.map(Into::into)`.
- Bind arguments use `t.to_sqlx()` (`ToSqlx`).
- Public `df-db` functions take and return `jiff::Timestamp`.

Other crates need `jiff-sqlx` too, but only as a dev-dependency for the test code listed in Context, and as a normal dependency of `df-web` for `auth/store.rs`.

Bazel: `df-db` already passes a `.env` to the macros through `compile_data` next to the manifest (`sqlx_dotenv`); `sqlx.toml` is read from the same directory and is added the same way. A throwaway spike checks this before the migration (task 2.2).

As built (`df-db`): nullable timestamps cannot go through `.into()` (`Option<jiff::Timestamp>: From<Option<jiff_sqlx::Timestamp>>` does not exist and the orphan rule forbids it), so the private record types (`RunRecord`, `ChatRecord`, `Row`, `TaskListRow`, ...) keep `Option<jiff_sqlx::Timestamp>` for nullable columns and the `TryFrom` converts with `.map(Into::into)`, while NOT NULL fields of `query_as!` targets are plain `jiff::Timestamp`. `FromRow` structs use `#[sqlx(try_from = "jiff_sqlx::Timestamp")]` for NOT NULL fields (`ArtifactMeta`, `RequestRecord`); `try_from` has no `Option` form, so their nullable fields stay wrapped as well. `sqlx.toml` and `compile_data` were not changed beyond D2; `.sqlx/` has no diff. The test-util module `df-orchestrator/src/testing.rs` converts in its helper (`TaskResumeState.resume_at` is a `jiff::Timestamp`). The `df-web` OIDC test provider builds its ID token claims from JSON, because `IdTokenClaims::new` takes `chrono` times and `df-web` must not depend on `chrono`.

As built (spike, task 2.2): a throwaway crate with its own `sqlx.toml`, one `query!` reading `blobs.created_at` and one `query_scalar!` binding a `jiff_sqlx::Timestamp`, using the query texts of `.sqlx/` (no database needed), compiled with `SQLX_OFFLINE=true` under cargo and under `bazel build` with `sqlx.toml` in `compile_data`. Both picked up the override (a `jiff_sqlx::Timestamp` result and bind type-checked while the `sqlx` feature `chrono` was still on, so the override takes precedence over the default mapping); `sqlx-toml` is active in the crate_universe build. The fallback is not needed.

Fallback, if `sqlx.toml` does not work under Bazel: column overrides for outputs (`created_at AS "created_at: jiff_sqlx::Timestamp"`) and `expr as _` for bind parameters, in every query. The query texts change, so `.sqlx/` is regenerated.

### D3. Protobuf timestamps: functions in `df-proto`

`pbjson_types::Timestamp` has `From`/`TryFrom` only for `chrono`. Neither `jiff::Timestamp` nor `pbjson_types::Timestamp` is local to `df-proto`, so the orphan rule forbids a `From` impl. `df-proto` gets a module `time` with two functions:

- `to_proto(jiff::Timestamp) -> Timestamp`. `jiff` gives a negative `subsec_nanosecond()` before 1970, protobuf requires `nanos` in `0..1_000_000_000`: if nanos < 0, use `seconds - 1` and `nanos + 1_000_000_000`.
- `from_proto(&Timestamp) -> Option<jiff::Timestamp>`: `None` for `nanos` outside `0..1_000_000_000` or seconds out of `jiff`'s range.

They replace `DateTime::from_timestamp(t.seconds, t.nanos)` in `df-db`, `df-agent-channel`, `df-gateway-control` and `df-web`, and `Utc::now().into()` in `factory-agent`. `pbjson-types` keeps its own `chrono` dependency (proposal).

### D4. Text formats

Kept byte for byte, pinned by tests written while the code is still on `chrono` (tasks group 1):

- Lifecycle event `timestamp`: RFC 3339, UTC, `Z`, three fractional digits (`format!("{t:.3}")`).
- Visible UI times: the `strftime` patterns (`%b %d %H:%M`, `%b %d %H:%M UTC`, `%b %d`, `%H:%M`, `%H:%M:%S`, `%Y-%m-%d %H:%M:%S UTC`).

Changed, both forms valid RFC 3339:

- `to_rfc3339()` prints `+00:00`, `jiff` prints `Z`. This changes the `<time datetime>` attribute in the web UI (no script reads it) and the `updated` value in the metadata line of the agent artifact tools (`tools/artifacts.rs`). Keeping `+00:00` would need a custom format string at each site for no benefit.
- `serde` JSON of domain types and `next_since` of `read_pull_request_feedback`: `chrono` prints 0, 3, 6 or 9 fractional digits, `jiff` the fewest needed. Both read any RFC 3339 value with offset, so stored JSON written by `chrono` stays readable, and `next_since` is only parsed back by the tool.

The pin tests for the changed texts are updated in the same task as the swap; the review sees the expected `+00:00` → `Z` diff.

As built: `RunReport.retry_after` (factory-agent) now carries sub-second precision, where it used to be whole seconds; the control plane reads it through `from_proto` into `timestamptz` and the visible resume text is unchanged.

As built: the two insta snapshots of `df-web` (`renders_every_event_kind`, `tool_call_before_its_update_renders_the_collapsed_summary`) changed only in `datetime` attributes (`+00:00` → `Z`); the `next_since` pin for a `since` of `…10:15:00.900Z` now reads `…10:15:00.9Z`. The pin tests of group 1 cover the lifecycle event timestamp, the artifact metadata line, the helper functions of the views that were private (`short_time`, shared in `components::time` for the chat list, task list, task run header and request cards; `format_time`, `time`, `time_label`, `run_divider`), the archive label (`%b %d`, helper `archived_label` in `chat_pages/archive.rs`, the only site of that pattern) and the last delivery of `project_subscriptions.rs` (`%b %d %H:%M UTC`), and the existing `next_since` tests. The `datetime` attribute is pinned at the sites with a test (transcript divider, artifact time, archive label, last delivery, the task resume time); the other sites build it with the same `.to_string()`. `egress/log/rows.rs` (`time_label`) now shows an empty label instead of a time for a proto timestamp with negative nanos (malformed input; `from_proto` returns `None`, `chrono` code used `unwrap_or(0)`).

`jiff` parsing accepts more than RFC 3339 (RFC 9557 annotations, lowercase `t`/`z`). The `since` argument keeps its scenario "no parsable timestamp"; a stricter check is not needed because the value is only a lower bound.

### D5. Keep `chrono` out: `cargo deny` wrapper ban

`deny.toml` `[bans] deny` gets `chrono` with `wrappers` set to the crates that still require it after the swap: `rmcp`, `schemars`, `openidconnect`, `oauth2`, `pbjson-types`, plus any other that `cargo tree -i chrono` shows then. The `sqlx` crates are not in the list: without the feature they must not pull `chrono`. A workspace crate (or a new third-party crate) that adds `chrono` fails `cargo deny check`. The `RUSTSEC-2026-0543` ignore stays, with its comment updated.

`tools/xtask` is a separate workspace that `cargo deny` does not check; it moves to `jiff` in the same change (a few lines), and the final grep covers its manifest.

Alternative: a `clippy.toml` `disallowed-types` entry. Rejected: it catches use, not the dependency, and needs one line per type.

### D6. One pass, crate by crate along the dependency order

`df-core`'s domain types are shared by all crates, so a mixed state would need conversions at every boundary. The migration is one change, done bottom-up: `df-proto` (D3) → `df-core` → `df-db` (D2) → `df-orchestrator`, `df-gateway-control` → `df-agent-channel` → `df-web` → `dark-factory`, `factory-agent`. After each step, the migrated crate and its dependencies compile and pass their tests; crates further up stay broken until their step, and the workspace builds again after the last one. The WIP commits are squashed at the end.

The pin tests (D4) therefore run first, as their own group, while the whole workspace is still on `chrono`.

## Risks / Trade-offs

- [`sqlx.toml` not picked up under Bazel] → the spike (task 2.2) checks it in a throwaway crate first; the fallback in D2 needs no configuration.
- [Override key typo or case mismatch fails silently while the `chrono` feature is still on] → the `chrono` feature is removed in the same step as `sqlx.toml` is added (task 4.1), so a wrong key is a compile error.
- [Changed RFC 3339 texts (D4)] → valid RFC 3339; consumers parse, they do not compare text; no spec pins them.
- [`Timestamp + SignedDuration` panics on overflow, like `chrono`'s `+`] → keep the existing `checked_*` calls where they exist.
- [`chrono` stays in the graph] → accepted (proposal).
- [Two `jiff` majors compiled] → `multiple-versions = "warn"` only.
- [Large mechanical diff conflicts with parallel changes in `df-db` and `df-web`] → land it in a quiet window; the edits are type renames, so rebasing is mechanical.

## Migration Plan

No data or deployment migration: column types and stored values do not change. Rollback is a revert of the change.

## Open Questions

None.
