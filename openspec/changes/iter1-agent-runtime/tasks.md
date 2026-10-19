# Tasks

## 1. Spike (iteration-1 §7)

- [x] 1.1 Spike `@zed-industries/claude-agent-acp` headless in the Claude project image with `CLAUDE_CODE_OAUTH_TOKEN`: `session/new` with an MCP server, `session/load` across process restarts, whether usage is reported, how to append a system prompt, how a rate limit surfaces, whether `HTTPS_PROXY` is honoured and which hosts are contacted; record findings in `crates/factory-agent/docs/claude-acp.md` and adjust design D5

## 2. Fake ACP agent

- [x] 2.1 Create `fake-acp-agent` with the script format (design D6) and steps `say`, `think`, `tool_call`, `plan`, `usage`, `end`; verify with an ACP client test that a scripted turn produces the expected updates and stop reason
- [x] 2.2 Add `shell` and `mcp` steps (stdio MCP client to servers from `session/new`); verify the "commit in a turn" and "factory tool call" scenarios
- [x] 2.3 Add rate limit (mirroring spike 1.1), crash, hang-until-cancel and `session/load` with per-session scripts; verify each scenario in `specs/fake-acp-agent/spec.md`

## 3. Connection

- [x] 3.1 Create `factory-agent` with subcommands `run`, `mcp`, `git-credential` and a test harness running a scripted control plane (tonic server with `rcgen` certs); verify mTLS connect with server-name verification
- [x] 3.2 Implement the outbox (design D3) test-first: seq assignment, persist-before-send, ack trimming, bound with back-pressure; verify with unit tests incl. restart recovery from disk
- [x] 3.3 Implement Hello/resume, replay after `last_persisted_seq`, run-id mismatch discard with notice, reconnect backoff and certificate file reload; verify the scenarios in `specs/runtime-connection/spec.md` (incl. control plane restart mid-run)
- [x] 3.4 Implement idempotent command handling and re-sending pending tool/credential requests; verify "prompt re-sent after reconnect"

## 4. Git and workspace

- [x] 4.1 Implement the Git command builder and `.git/config` rendering (design D4) test-first; verify "planted configuration", "malicious hook" and "non-HTTPS remote"
- [x] 4.2 Implement the runtime socket and `factory-agent git-credential` (get/store/erase); verify a clone from a local HTTPS test server (`git http-backend` behind a TLS test server) uses the helper and no password lands under the test's `/data`
- [x] 4.3 Implement prepare (fresh clone, existing workspace, unpushed commits, fast-forward, divergence, WIP restore); verify every scenario of the prepare requirements in `specs/workspace-lifecycle/spec.md` with local bare repos
- [x] 4.4 Implement push-after-turn with `PushReport` and finalize (harness stop, config rewrite, push, WIP snapshot via separate index with denylist and size cap, `RunReport`); verify the push, finalize and `.env` scenarios, and that `git status`/HEAD/index are unchanged by the snapshot

## 5. ACP harness and MCP server

- [x] 5.1 Implement harness start (settings, env, cwd, HOME, no fs/terminal capabilities), `session/new`/`session/load` with summary fallback and MCP server registration; verify with `fake-acp-agent` the "resume" and "resume fails" scenarios and that the token is not written under `/data`
- [x] 5.2 Implement turn-based prompt delivery with `TurnStarted`/`UserMessage` and auto-approved permission requests; verify "two queued messages"
- [x] 5.3 Implement ACP → transcript translation (message ids, tool calls/updates, origin, truncation, plans, `TurnEnded`) test-first with recorded ACP update fixtures; verify "text after a tool call" and "large tool output"
- [x] 5.4 Implement usage capture (adapter or session-log parser per design D5); verify with a fixture session JSONL
- [x] 5.5 Implement `factory-agent mcp` (rmcp stdio server ↔ runtime socket), `tools/list` from `Assign.tools`, forwarding with UUIDv7 `call_id`; verify the scenarios in `specs/factory-mcp-server/spec.md`
- [x] 5.6 Implement run outcome determination, `end_run_after_turn`, `Cancel` handling and local budgets; verify every outcome scenario in `specs/acp-harness/spec.md` using fake-agent scripts

## 6. Verification

- [x] 6.1 Run the full runtime locally (scripted control plane, local HTTPS Git server, `fake-acp-agent`): two runs on one `/data` directory — run 1 commits and ends `END_TURN`, run 2 loads the session, commits and calls `complete_task`; verify the transcript, push reports and run reports
- [x] 6.2 Run `factory-agent` with the real Claude adapter against a scratch repository using the scripted control plane (manual, subscription token from env); record the transcript sample in `crates/factory-agent/docs/claude-acp.md`
- [x] 6.3 Ship `hooks-empty/` and `gitconfig` under `crates/factory-agent/image/` for the runtime image, and verify `cargo llvm-cov -p factory-agent -p fake-acp-agent` reports ≥ 70 % line coverage
