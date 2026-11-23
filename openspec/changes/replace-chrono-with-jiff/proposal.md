# Proposal

## Why

[RUSTSEC-2026-0543](https://rustsec.org/advisories/RUSTSEC-2026-0543) marks `chrono` as unmaintained (all versions, no fixed release, [announcement](https://github.com/chronotope/chrono/issues/1768)). `cargo deny check` fails on it, so `deny.toml` ignores it since commit `45129a2`. Our crates use `chrono` for every point in time and every duration, and `iter3-routines` was about to add two more `chrono` crates (`croner` feature `chrono`, `chrono-tz`). Moving our code to `jiff` now, before MVP, stops new code from building on an unmaintained library and gives us a maintained timezone database for routines.

What this change cannot do: clear the advisory. Of the dependencies that use `chrono`, four require it with no feature to turn it off, in their latest releases (checked 2026-11-23):

| Dependency (version in `Cargo.lock`) | `chrono` | Way out |
|---|---|---|
| `sqlx-core`, `sqlx-postgres` 0.9.0 | optional, feature `chrono` | drop the feature; `jiff-sqlx` 1.0 adds `jiff` types for `sqlx` 0.9 |
| `schemars` 1.2.3 | optional, feature `chrono04` | enabled by `rmcp`, not by us |
| `rmcp` 3.11.0 | required | none |
| `openidconnect` 4.1.0, `oauth2` 5.0.0 | required | none |
| `pbjson-types` 0.9.0 | required | none |

So the `RUSTSEC-2026-0543` ignore stays. After this change, `chrono` is only a transitive dependency.

## What Changes

- Our crates use `jiff` 1.0 instead of `chrono`: `jiff::Timestamp` for points in time (today `DateTime<Utc>`), `jiff::SignedDuration` for durations (today `TimeDelta` / `chrono::Duration`), `jiff` `strftime` and RFC 3339 formatting in the web UI, the agent tools and the lifecycle events.
  Crates: `dark-factory`, `df-core`, `df-db`, `df-orchestrator`, `df-agent-channel`, `df-gateway-control`, `df-web`, `factory-agent`, `df-proto` (new conversions between `jiff::Timestamp` and `pbjson_types::Timestamp`), and the standalone `tools/xtask`.
- `df-db` reads and writes `timestamptz` through `jiff-sqlx` (`jiff_sqlx::Timestamp`), mapped for all `query!` macros by `crates/df-db/sqlx.toml`. The `sqlx` feature `chrono` is replaced by `sqlx-toml`.
- `chrono` is removed from `[workspace.dependencies]` and from every manifest. A `cargo deny` ban keeps it from coming back as a direct dependency of a workspace crate.
- `iter3-routines` is updated in the same change set and now depends on this change: `croner` with the `jiff` feature, timezones as `jiff::tz::TimeZone` from the bundled tzdb, no `chrono-tz`.
- The backlog row in `product/iterations/roadmap.md` is narrowed to what is left: drop the `deny.toml` ignore once `rmcp`, `openidconnect`, `oauth2` and `pbjson-types` no longer require `chrono`.
- Behavior, configuration and database schema do not change. Text formats:
  - unchanged: visible UI times, lifecycle event `timestamp` (milliseconds, `Z`);
  - changed, both valid RFC 3339: the `<time datetime>` attribute in the web UI and the `updated` value in the metadata line of the agent artifact tools (`tools/artifacts.rs`) print `Z` instead of `+00:00`; `serde` JSON and `next_since` print the fewest fractional digits instead of 0, 3, 6 or 9 (design D4). No spec pins these texts.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. Library swap without a requirement change: `skip_specs: true` in `.openspec.yaml`.

## Out of Scope

- Removing the `RUSTSEC-2026-0543` ignore in `deny.toml`. Blocked by `rmcp`, `openidconnect`, `oauth2` and `pbjson-types`. Target: backlog row "Replace `chrono` …" in `product/iterations/roadmap.md`, narrowed by this change.
- Replacing `pbjson-types` with `prost-types` and our own JSON mapping to drop its `chrono` dependency. Rejected: more code than the advisory is worth while `rmcp` and `openidconnect` keep `chrono` in the graph anyway.
- Replacing `time` (used by `tower-sessions`, `cookie`, `rcgen`, `x509-parser`) with `jiff`. Rejected: `time` is maintained, and we only touch it at the `web_sessions` store boundary.
- Showing times in the user's timezone in the web UI. Rejected for now: no user asked for it; the UI stays UTC.
- Moving `kube`, `k8s-openapi` and `gix-date` from `jiff` 0.2 to 1.0. Rejected: not ours to change; the second `jiff` major only gives a `multiple-versions` warning.

## Impact

- Code: about 170 `DateTime<Utc>` uses, 30 duration uses and 20 format calls across the crates listed above, mostly mechanical. Test code that binds or reads `timestamptz` with runtime `sqlx` queries outside `df-db` also changes (dev-dependency `jiff-sqlx`).
- Configuration: `crates/df-db/sqlx.toml` (design D2), added to `compile_data` of the `df-db` Bazel targets. `.sqlx/` metadata does not change (the query texts stay the same).
- Dependencies: `jiff` 1.0 and `jiff-sqlx` 1.0 become workspace dependencies; `sqlx` gains feature `sqlx-toml` (adds `toml` 0.8); `chrono` is removed. `jiff` 1.0 is in `Cargo.lock` today but not compiled; after this change both `jiff` 0.2 (through `kube`, `gix-date`) and 1.0 are compiled. `Cargo.lock`, `tools/xtask/Cargo.lock` and `MODULE.bazel.lock` change.
- Contributor docs: skill `rust` ("Database access", dependency example).
- Published docs: none. Nothing a user or operator configures changes.
- Order: lands before `iter3-routines`, which builds on `jiff` and `jiff-sqlx` from the start.
