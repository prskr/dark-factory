# Design

## Context

See proposal.md, "Why". The 17 functions, measured by `clippy::too_many_lines` (comments and blank lines do not count), and the split named in each attribute:

| Function | Lines | Split named in `reason` |
|---|---|---|
| `factory-agent` `runner.rs` `run_session` | 743 | setup, turn loop, finalize |
| `fake-acp-agent` `agent.rs` `build_agent` | 374 | one builder per scripted handler |
| `df-k8s` `render.rs` `render_sandbox` | 289 | one builder per resource section |
| `factory-agent` `connection.rs` `run_loop` | 277 | connect, send, receive |
| `factory-agent` `workspace.rs` `prepare_workspace` | 213 | fresh clone, existing workspace sync |
| `df-k8s` `render.rs` `render_network_policy` | 200 | ingress and egress rule builders |
| `df-agent-channel` `tools.rs` `run_handler` | 161 | database effects, forge call |
| `df-agent-channel` `session.rs` `persist_batch` | 152 | dedupe, gap check, apply |
| `factory-agent` `main.rs` `main` | 150 | argument parsing, setup, run |
| `df-web` `egress/log/page.rs` `state` | 149 | Alpine script to a static JS file |
| `factory-agent` `workspace.rs` `finalize_workspace` | 141 | config rewrite, push, WIP snapshot, report |
| `df-web` `transcript/mod.rs` `apply_event` | 135 | one handler per event kind |
| `dark-factory` `main.rs` `KubernetesBackend::start` | 120 | startup check, wiring, spawn |
| `df-db` `tasks.rs` `create_task` | 115 | numbering, insert, prompt enqueue, notify |
| `df-k8s` `provider.rs` `resolve_cluster_target` | 113 | Service lookup, named port resolution |
| `df-k8s` `reconcile.rs` `reconcile_active_runs` | 106 | list, diff, apply |
| `df-db` `requests.rs` `decide` | 104 | validation, state update, audit |

Line numbers move with every commit, so the table names functions, not lines.

## Goals / Non-Goals

**Goals:** every production function at or under 100 lines; the steps readable on their own; behavior unchanged.

**Non-Goals:**
- Splitting test functions. Test modules keep their module-level opt-out.
- Files of 1,000 to 1,500 lines. They are under the file limit.
- Lint fixes of `clippy-strictness`.
- Changing public crate APIs beyond what a split needs. Helpers stay private or `pub(crate)`.

## Decisions

### D1. Safety net first

Before a function is split, the implementer lists the tests that exercise it: the module's tests, integration tests, and the `RunScenario` scenarios for `run_session`.

- A branch of the function that no test reaches gets a characterization test first, in the same group. It asserts today's behavior, not the desired one.
- A bug found on the way is reported, not fixed silently. The fix belongs in its own commit, with its own regression test.

### D2. The `reason` is a starting point, not a contract

The split in the attribute was written without the full context. The implementer may choose different seams if they read better, and states the chosen steps in the commit message. Rules for the result:

- **Step names:** each step has one purpose that its name states (`verify_cluster`, `spawn_background_tasks`), not `part1` or `helper`.
- **Shared state:** when steps share more than about five values, they share a small struct (Data Clumps) instead of long parameter lists. Typical cases are `run_session` and `run_loop`. A struct field that only one step uses stays a local of that step.
- **Visibility:** steps are private functions next to the original, or methods on that struct.
- **Moves and changes in separate commits:** a group commits code moves (to a new module or file) separately from changes in logic, so `git diff --color-moved` shows them apart.

### D3. `run_session` gets its own group

At 743 lines, `run_session` holds setup (workspace prepare, harness start, ACP handshake), the turn loop, the idle wait and finalization, all sharing state. It is split in steps:

1. Extract a run state struct that holds the values shared across phases.
2. Extract setup and finalize, the parts at the edges.
3. Extract the turn loop and the idle wait.

Each step is a commit, and the `factory-agent` tests pass after each one. `run_session` itself becomes a short sequence of phase calls.

### D4. Traffic page Alpine component as a static asset

- **Move:** the component's JS moves to `crates/df-web/assets/egress-log.js` as `Alpine.data('egressLog', (init) => ({ … }))`. The page includes the script and passes the server values (`advanced`, `dirty`, the advanced field list, `WIDTH_KEY`) as a JSON `x-data` argument, escaped the way the page escapes other attributes today.
- **Benefits:** the Rust function shrinks to building that JSON, and the JS becomes editable as JS.
- **Bazel:** the build already stages `assets/` into `OUT_DIR` and embeds it. The implementer checks that a new file reaches the embed in both cargo and Bazel, through the `data`/`compile_data` of `df-web`'s `BUILD.bazel`.
- **Check:** an existing page test, or a new one, asserts that the page references the script and that the asset route serves it.

### D5. `dark-factory/src/main.rs` module split

The sandbox backend wiring moves to `crates/dark-factory/src/sandbox.rs`: `SandboxBackend`, `SandboxWiring`, `UnconfiguredBackend`, `KubernetesBackend`, `DbClusterTargetsProvider`, `K8sServices`, `RelayFlows` and `resolve_cluster_ips`.

- **Tests:** the `K8sServices` namespace test moves with it.
- **Split of `KubernetesBackend::start`:** startup check and endpoint resolution, provisioning config, the notification forwarder, and the background tasks.
- **Order:** the move is a commit of its own, before the split (D2).

## Risks / Trade-offs

- **Subtle behavior changes in async control flow.** These include the order of `select!` branches, a lock or guard that now lives across a different `.await`, and drop order. → A split must not change the order of awaits or the lifetime of guards. The tests run after every commit. The reviewer checks the `select!` and lock rules of AGENTS.md, "Concurrency".
- **Struct extraction can produce a "god struct".** → D2: fields only for state that steps share. The reviewer looks for Data Clumps and fields that only one step uses.
- **Container-backed tests** (`df-agent-channel`, `df-db`) need the Podman socket. → Run them crate by crate with limited parallelism (`-j2`, `--test-threads=2`). The Podman VM died under full parallel load in `split-oversized-test-modules`.
