## Context

See proposal.md, "Why". Current state:

- `clippy.toml` bans the `sqlx` runtime query functions, the `sqlx::Executor` methods, the three query macros and `sqlx::QueryBuilder` workspace-wide, so that other crates go through `df-db`.
- `crates/df-db/src/lib.rs` opts out of `clippy::disallowed_methods` and `clippy::disallowed_macros` at crate level. `disallowed_types` is not opted out: `df-db` uses no `QueryBuilder`.
- clippy has no baseline: once `disallowed_methods` leaves the crate-level `expect`, every existing runtime call fails `cargo clippy --all-targets --all-features -- -D warnings` (pre-push hook in `lefthook.yml`, CI) and the Bazel lint run (`build:lint` in `.bazelrc`, which passes `//:clippy.toml` to the clippy aspect).

Runtime calls in production code, recounted by grep on `main` at `6a75601` (the handover's counts from `feat/question-kinds` missed the turbofish form `sqlx::query_as::<_, T>(` and the `Executor` methods). Code from the first `#[cfg(test)]` in a file to its end, and `*tests.rs` files, are left out. Task 1.1 replaces this list with clippy's.

| File | Calls | Lines | Kind |
|---|---|---|---|
| `artifacts.rs` | 7 | 77, 91, 134, 149, 165, 174, 190 | static |
| `egress.rs` | 1 | 452 (`pg_advisory_xact_lock`) | static |
| `migrate.rs` | 7 | 49, 59 (migration lock), 104, 126 (`pg_roles`); 83, 139, 177 (`Executor::execute` with `AssertSqlSafe`) | 4 static, 3 dynamic DDL |
| `model_providers.rs` | 14 | 106, 116, 143, 164, 207, 224, 231, 256, 272, 303, 320, 343, 389, 402 | static; 164 picks one of two literals, 106/116/143 use the `select_provider!` `concat!` macro |
| `model_usage.rs` | 2 | 46, 70 | static |
| `notify.rs` | 1 | 278 (`pg_notify($1, $2)`) | static |
| `pool.rs` | 1 | 115 (`SET search_path`, `Executor::execute`) | dynamic |
| `requests.rs` | 12 | 565, 617, 626, 726, 764, 778, 798, 814, 867, 884, 895, 958 | static; several use `concat!` |
| `task_events.rs` | 15 | 181, 244, 255, 283, 287, 298, 302, 317, 387, 409, 428, 432, 450, 506, 537 | static; 244 picks one of two literals |
| `tasks.rs` | 14 | 66, 297, 358, 551, 686, 715, 1040, 1117, 1193, 1231, 1263, 1267, 1321, 1345 | static |
| `webhooks.rs` | 1 | 121 | static |

`index_predicates.rs` (in the handover's list) is compiled only under `#[cfg(test)]` (`lib.rs`), so it is test code. Test-only runtime calls are in many more files (for example `users.rs`, `outbox.rs`, `projects.rs`, `task_views.rs`, `chats/tests.rs`).

## Goals / Non-Goals

**Goals:**

- A runtime query in new `df-db` production code fails the lint run, with no new tool.
- Each existing runtime query sits under an opt-out that says why, and an opt-out disappears once its file is converted.

**Non-Goals:**

- Converting queries to macros: backlog row "Checked queries in `df-db`: convert the files with runtime queries" (`product/iterations/roadmap.md`).
- `sqlx-cli` in `bazel_env`: backlog row "`sqlx-cli` in `bazel_env`" (`product/iterations/roadmap.md`).
- Stricter rules for other crates: unchanged, they already fail on any query.

## Decisions

### D1: Ratchet with `expect` attributes, not a baseline tool

The crate-level opt-out drops `disallowed_methods`; each file that still has runtime calls carries its own `#![expect]`. `expect` (not `allow`, which `clippy::allow_attributes` forbids anyway) fails when nothing in its scope triggers the lint, so a converted file must delete its opt-out.

Alternatives: convert all ~75 calls now (needs a database and `cargo sqlx prepare`, which a sandbox cannot run yet, and changes behavior-relevant code in one go); an `xtask` grep check with a list of allowed call sites (a second lint tool next to clippy, line-number churn, misses calls clippy sees through imports). Both rejected.

### D2: Item-level opt-out for dynamic SQL right away

Answer to the handover's first open question. Only four functions build SQL at runtime, all through `sqlx::Executor::execute` with `sqlx::AssertSqlSafe` and identifiers checked by `validate_sql_identifier`:

| Function | Reason |
|---|---|
| `migrate::run_migrate_locked` | `"dynamic SQL: CREATE SCHEMA with a validated identifier"` |
| `migrate::manage_application_role` | `"dynamic SQL: CREATE/ALTER ROLE with a validated identifier"` |
| `migrate::apply_grants` | `"dynamic SQL: GRANT/REVOKE with validated identifiers"` |
| `pool::connect_pool` | `"dynamic SQL: SET search_path with a validated identifier"` |

So no file-level opt-out says "dynamic SQL". `pool.rs` gets only the item-level one. `migrate.rs` gets both: the item-level ones, and a file-level `"unchecked queries, not yet converted"` for the migration lock calls (49, 59) in `migrate`. The `pg_roles` queries (104, 126) sit inside `run_migrate_locked` and `manage_application_role` and are covered by their item-level `expect` (see Risks).

The two "pick one of two literals" calls (`model_providers.rs:164`, `task_events.rs:244`) are not dynamic: each can become two macros. They stay under "not yet converted".

### D3: The `Executor` methods count

Answer to the handover's second open question: yes. `migrate.rs` (3 calls) and `pool.rs` (1) pass SQL text to `Executor::execute` directly. `.execute(...)` on a `Query` built by a macro is `sqlx::query::Query::execute`, not the trait method, and is not hit. Task 1.1 confirms with clippy.

### D4: Test code through the crate root

`#![cfg_attr(test, expect(clippy::disallowed_methods, reason = "test fixtures"))]` in `lib.rs`, as `AGENTS.md` prescribes for test code at crate roots. Under `cfg(test)` it is fulfilled by the test modules of files without a file-level opt-out (for example `users.rs`, `chats/tests.rs`). Test modules of the 11 files are covered by their file's opt-out instead. `df-db` has no `tests/` directory, so no integration test file needs its own `#![expect]`.

### D5: Reason strings are fixed

Two forms only, so a grep finds what is left: `"unchecked queries, not yet converted"` and `"dynamic SQL: <what>"`. `AGENTS.md` names both.

## Risks / Trade-offs

- [A new runtime call in one of the 10 files with a file-level opt-out passes clippy] → Reviewers keep watching these files; the standards reviewer reads `AGENTS.md`, which names the ratchet. The conversion follow-up closes the gap file by file.
- [The item-level opt-outs in `migrate.rs` also hide the static `pg_roles` queries (104, 126) and any new runtime call in those three functions] → The conversion follow-up moves them to macros; the functions are short and change rarely.
- [Under `cfg(test)`, the crate-root `expect` becomes unfulfilled if every test-only runtime call outside the 11 files is converted] → Intended: the lint run then asks to delete it.
- [The Bazel clippy aspect could read inner attributes differently] → It runs the same clippy on the same sources with the same `clippy.toml`; task 3.2 checks it once.
- [The grep count may miss calls (multi-line paths, imported functions)] → Task 1.1 takes the list from clippy, not from grep.
