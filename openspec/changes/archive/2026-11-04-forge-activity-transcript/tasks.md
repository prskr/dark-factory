# Tasks

## 1. Contract and storage

- [x] 1.1 Add `ForgeActivity`, `BranchPushed`, `PullRequestOpened`, `PullRequestReady` and `PushVerdict` to `contract/proto/darkfactory/transcript/v1/transcript.proto` as `Event.kind` field 19 (design D1), document them in `contract/README.md`; verify `buf format` passes and `cargo build -p df-proto` generates the types
- [x] 1.2 Write the failing `df-db` tests for D2 (duplicate `(run, source_key)` stores one row; a row with both or neither of `seq` and `source_key` is rejected), then add the migration; verify with `cargo test -p df-db transcript` (container-backed)
- [x] 1.3 Find readers that assume a non-null `seq` (`grep -rn "seq" crates/*/src` plus compile errors of `query!` macros) and fix them; verify `cargo build --workspace` and `cargo clippy --all-targets --all-features -- -D warnings`
- [x] 1.4 Write failing tests for `df_db::transcript::append_control_event` (stores canonical JSON with kind `forge_activity`, returns `None` for a duplicate key, publishes `transcript_event` with the new id only when inserted; with the task row locked by another transaction it waits, so ids become visible in order), then implement it with the run-then-task locks (design D3); regenerate `.sqlx/`; verify the tests pass
- [x] 1.5 Write a failing `df-agent-channel` test that a `ForgeActivity` sent by the runtime is not stored while its following events are, then skip it in `transcript::prepare` with a warning (design D6); verify the test passes (container-backed)

## 2. Push activity

- [x] 2.1 Write a failing `df-gateway` test that a push whose task ref the forge accepts while it refuses another ref records `accepted` for the task ref and `upstream-failed` for the refused one, and that an unreachable forge fails every ref; then record per-ref verdicts in `Entry::record` (design D4); verify `cargo test -p df-gateway`
- [x] 2.2 Write failing `git_pushes` tests: accepted task-branch push writes one `BranchPushed` with ref, SHAs and the ref's own verdict; rejected push carries the ref's reason; `forwarding` writes nothing; WIP-only push writes nothing; the same final verdict twice writes one event (spec `forge-activity`)
- [x] 2.3 Append `BranchPushed` in `record_push` for the run's task branch (design D4); regenerate `.sqlx/`; verify the 2.2 tests pass
- [x] 2.4 Add a test in `df-agent-channel` that an OK `PushReport` without a push record writes no forge activity; verify it passes

## 3. Pull request activity

- [x] 3.1 Write failing `pr.rs` tests: first push ahead opens draft PR and writes one `PullRequestOpened` (draft) in that run; an existing open PR found and recorded writes one event; concurrent push report and run end write one event
- [x] 3.2 Pass the run id into `record_pr`, take the run-then-task locks before its task update, and append `PullRequestOpened` when the PR is recorded (design D3, D5); verify the 3.1 tests pass
- [x] 3.3 Write failing tests: completed run marking PR ready writes one `PullRequestReady`; a PR opened at run end gets a ready event with its URL; a failed ready update writes none; then return number and URL from `try_ensure_draft` and append the event in `on_run_end` after a successful update; verify the tests pass

## 4. Transcript rendering

- [x] 4.1 Add Lucide `git-commit-horizontal` and `git-pull-request-draft` as `Icon::GitCommit` and `Icon::PullRequestDraft`; list them in `docs/design-system.md` (Icons); verify the icon tests in `df-web` pass
- [x] 4.2 Write failing `df-web` transcript tests for each row of the design D8 table (icon, text, tint), the non-`http(s)` URL rendered without a link, and divider sharing with tool and notice rows; then add `ItemKind::Forge` to the folder and renderer; verify `cargo test -p df-web transcript`
- [x] 4.3 Add a test that `assign::transcript_summary` ignores `forge_activity` rows (spec "Forge activity is not sent to the agent"); verify it passes (container-backed)
- [x] 4.4 Add an SSE test that a control-plane forge activity event reaches a connected transcript stream in order; verify with `cargo test -p df-web sse` (container-backed)
- [x] 4.5 Document the forge activity row in `docs/design-system.md` (transcript rows) next to notices; verify the doc names all three icons and the link rule

## 5. Integration

- [x] 5.1 Run `bazel --bazelrc=.bazelrc.agents build //...`, `cargo xtask check-file-length`, `cargo machete` and the pre-push clippy; verify all pass
- [x] 5.2 With a container socket, run `just serve-local`, push to a task branch and complete a run with the fake agent; verify push, draft PR and ready rows appear live in order in the browser (Done as: the control plane ran against a throwaway database and the push, PR and ready rows were inserted by SQL + NOTIFY while the page was open; they appeared live and in order. The gateway -> `record_push` -> `pr.rs` path is covered by the `df-db`, `df-gateway` and `df-agent-channel` tests, not exercised end to end.)

## Workflow follow-up

- Archive the change after review; confirm `openspec/specs/forge-activity/spec.md` is created and `product/GLOSSARY.md` still matches the archived specs.
