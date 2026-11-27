## Context

See proposal.md, "Why". Current state:

- `clippy.toml` bans the `sqlx` runtime query functions, the `sqlx::Executor` methods, the three query macros and `sqlx::QueryBuilder` workspace-wide, so that other crates go through `df-db`.
- `crates/df-db/src/lib.rs` opts out of `clippy::disallowed_methods` and `clippy::disallowed_macros` at crate level. `disallowed_types` is not opted out: `df-db` uses no `QueryBuilder`.
- clippy has no baseline: once `disallowed_methods` leaves the crate-level `expect`, every remaining runtime call fails `cargo clippy --all-targets --all-features -- -D warnings` (pre-push hook in `lefthook.yml`, CI) and the Bazel lint run (`build:lint` in `.bazelrc`, which passes `//:clippy.toml` to the clippy aspect).
- The checked macros expand to `sqlx::__query_with_result` / `__query_scalar_with_result` (`sqlx-macros-core` 0.9.0, `src/query/mod.rs`, `output.rs`), which are separate functions from the banned `query_with` (checked by Grace in DF-45 against the `sqlx` 0.9.0 sources). The `.execute()` / `.fetch_*()` calls on a macro's query are inherent methods of `sqlx::query::Query` and `Map`, not the `Executor` trait methods.

Runtime calls in production code, recounted by grep on `main` at `6a75601` and confirmed line for line by source reading in the review (DF-45). Code from the first `#[cfg(test)]` in a file to its end, and `*tests.rs` files, are left out. Task 1.2 confirms the list with clippy.

| File | Calls | Lines | Kind |
|---|---|---|---|
| `artifacts.rs` | 7 | 77, 91, 134, 149, 165, 174, 190 | static |
| `egress.rs` | 1 | 452 (`pg_advisory_xact_lock`) | static |
| `migrate.rs` | 7 | 49, 59 (migration lock, in `run_migrate`), 104, 126 (`pg_roles`); 83, 139, 177 (`Executor::execute` with `AssertSqlSafe`) | 4 static, 3 dynamic DDL |
| `model_providers.rs` | 14 | 106, 116, 143, 164, 207, 224, 231, 256, 272, 303, 320, 343, 389, 402 | static; 164 picks one of two literals, 106/116/143 use the `select_provider!` `concat!` macro |
| `model_usage.rs` | 2 | 46, 70 | static |
| `notify.rs` | 1 | 278 (`pg_notify($1, $2)`) | static |
| `pool.rs` | 1 | 115 (`SET search_path`, `Executor::execute`, in `connect_pool`) | dynamic |
| `requests.rs` | 12 | 565, 617, 626, 726, 764, 778, 798, 814, 867, 884, 895, 958 | static; several use `concat!` and the `RequestRecord` `FromRow` struct |
| `task_events.rs` | 15 | 181, 244, 255, 283, 287, 298, 302, 317, 387, 409, 428, 432, 450, 506, 537 | static; 244 picks one of two literals |
| `tasks.rs` | 14 | 66, 297, 358, 551, 686, 715, 1040, 1117, 1193, 1231, 1263, 1267, 1321, 1345 | static |
| `webhooks.rs` | 1 | 121 | static |

So 71 static calls in 10 files, and 4 dynamic calls in 4 functions. `index_predicates.rs` (in the handover's list) is compiled only under `#[cfg(test)]` (`lib.rs`), so it is test code. Test-only runtime calls are in many more files (for example `users.rs`, `outbox.rs`, `projects.rs`, `task_views.rs`, `chats/tests.rs`).

## Goals / Non-Goals

**Goals:**

- Every static query in `df-db` production code is a checked macro.
- A runtime query in any new or changed production function fails the lint run, with no new tool.
- What stays a runtime query carries an opt-out on its own function that says why.

**Non-Goals:**

- `sqlx-cli` in `bazel_env`: backlog row "`sqlx-cli` in `bazel_env`" (`product/iterations/roadmap.md`).
- Converting test fixtures to macros: rejected (test code stays exempt, `AGENTS.md`).
- Stricter rules for other crates: unchanged, they already fail on any query.

## Decisions

### D1: Convert, then opt out per function

The static calls become macros in this change (decided by Peter on the review, DF-45), so the only runtime queries left are the four dynamic functions. Each opt-out sits on the function: `#[expect(clippy::disallowed_methods, reason = "...")]`. `expect` (not `allow`, which `clippy::allow_attributes` forbids anyway) fails when nothing in its scope triggers the lint, so it cannot outlive its query.

Alternatives: file-level opt-outs for the unconverted files (the first draft): rejected in review (DF-45, finding 1), because a new function in such a file, like `close_pending` in `requests.rs`, would still pass. Opt-outs per function without converting: rejected by Peter in favor of converting. An `xtask` grep check with a list of allowed call sites: a second lint tool next to clippy, line-number churn; rejected.

### D2: What stays a runtime query

Only SQL that cannot be a macro:

| Function | Reason |
|---|---|
| `migrate::run_migrate_locked` | `"dynamic SQL: CREATE SCHEMA with a validated identifier"` |
| `migrate::manage_application_role` | `"dynamic SQL: CREATE/ALTER ROLE with a validated identifier"` |
| `migrate::apply_grants` | `"dynamic SQL: GRANT/REVOKE with validated identifiers"` |
| `pool::connect_pool` | `"dynamic SQL: SET search_path with a validated identifier"` |

The static `pg_roles` queries in `run_migrate_locked` and `manage_application_role` (104, 126) are converted too; the `expect` stays fulfilled by the `Executor::execute` call in each function.

Conversion rules for the static calls:

- A call that picks one of two literals (`model_providers.rs:164`, `task_events.rs:244`) becomes a `match` with one macro per arm.
- `concat!` inside a query macro (`select_provider!`, the `RequestRecord` queries in `requests.rs`): whether `sqlx` 0.9.0 accepts it is (unverified); task 1.3 checks it. If not, each query spells out its full literal.
- `query_as!` does not use `FromRow`; a `#[derive(FromRow)]` row struct used only by runtime queries gets field types that fit the macro (`Option<jiff_sqlx::Timestamp>` for nullable `timestamptz`, `AS "col!"` / `"col?"` / `"col: Type"` overrides), as skill `rust` describes, or the derive goes away when nothing else needs it.
- A static call that still cannot be a macro gets `#[expect(clippy::disallowed_methods, reason = "unchecked query: <why it cannot be a macro>")]` on its function and a row in the table above, with the reason. The review of the implementation checks each such row.

### D3: The reason text is a review rule, not a spec rule

Clippy accepts any `reason`, so the spec requires only the `expect` (DF-45, finding 2). `AGENTS.md` names the two forms, `"dynamic SQL: <what>"` and `"unchecked query: <why>"`, so a grep finds what is left and a reviewer can check them.

### D4: Ban the hidden query functions

`sqlx::__query_with_result` and `sqlx::__query_scalar_with_result` are `#[doc(hidden)]` re-exports that take SQL text (DF-45, finding 6). They join `disallowed-methods`. The checked macros expand to calls of these functions; clippy's `disallowed_methods` is expected to skip spans from external macro expansions (unverified). Task 1.2 checks that a `query!` does not trigger it.

Fallback if it does: drop the two entries, and change the spec in the same step. Delete the scenario "Hidden query function" and probe (c) of task 4.1, and add a Risks entry: a direct call of a hidden function passes clippy, and only review catches it. No other mechanism is added (no `xtask` grep, D1). Today nothing calls these functions by hand, and their `__` names mark them as internal.

### D5: Test code through the crate root

`#![cfg_attr(test, expect(clippy::disallowed_methods, reason = "test fixtures"))]` in `lib.rs`, as `AGENTS.md` prescribes for test code at crate roots. With no file-level opt-outs left, every test-only runtime call fulfils it, so it stays fulfilled while any test fixture runs a runtime query. `df-db` has no `tests/` directory and no features, so no integration test file needs its own `#![expect]` and `--all-features` adds nothing.

### D6: One file per commit, `.sqlx/` with it

Each file is converted in its own commit with its `.sqlx/*.json` files, so a review or a revert covers one file. Regeneration: `cargo sqlx prepare --workspace -- --all-targets` against a migrated database (`docs/development.md`, "Updating `sqlx` Offline Metadata"); it needs `sqlx-cli`, a PostgreSQL and a C compiler for the `cargo` build. Task 1.1 checks that before any conversion.

## Risks / Trade-offs

- [The implementer's sandbox cannot regenerate `.sqlx/` (no `sqlx-cli`, no container socket, no C compiler; the review sandbox of DF-45 had none of them)] → Task 1.1 checks it first and stops with a report instead of converting blind. A conversion without fresh `.sqlx/` breaks every offline build.
- [A macro infers a different nullability or type than the runtime query decoded, and a read fails at runtime] → Each converted function keeps a passing `df-db` test that reads its result (task per file); the tests run against PostgreSQL. Nullability overrides follow skill `rust`.
- [A new runtime call inside one of the four dynamic functions passes clippy] → The functions are short, change rarely, and the opt-out names what they do.
- [Under `cfg(test)`, the crate-root `expect` becomes unfulfilled if every test fixture is converted] → Intended: the lint run then asks to delete it.
- [The Bazel clippy aspect could read the attributes differently] → It runs the same clippy on the same sources with the same `clippy.toml`; task 4.2 checks it once.
- [A large diff (71 queries)] → One file per commit (D6); no behavior change is intended, and the existing tests are the guard.
