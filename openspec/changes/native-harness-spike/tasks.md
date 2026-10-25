# Tasks

Time box: 3 days in total (design, Risks). The prototype stays on the spike branch, nothing is merged into `main` apart from this change's `design.md` (Findings, Decision) and, if the decision is to build, the follow-up documents of group 4.

## 1. Baseline and source review

- [ ] 1.1 Measure Claude Code on its subscription in a sandbox with the fixed task from design D4 (3 runs): peak and steady RSS of the adapter and CLI tree, cgroup `memory.peak`, outcome, tokens, wall-clock time; verify the row "Claude Code" in `design.md` Findings is filled in with the measurement command used
- [ ] 1.2 Review the `rpi-ai`, `rpi-agent`, `rpi-tools` and `rpi-harness` sources against design D5 (streaming events vs. ACP updates, which reasoning field the spike endpoint uses and whether the `rpi-ai` OpenAI-compatible provider reads it (D3), cancellation, usage per turn, rate-limit errors, `ExecutionEnv` and session store as replaceable traits, MCP client, features and transitive dependencies with default features off); verify a "Source review" note per criterion in `design.md` Findings, each with file references
- [ ] 1.3 Stop point: if 1.2 shows a no-go on ACP fit, estimate the octolib variant instead (D2) and continue with 3.2; verify the decision to continue or switch is recorded in `design.md` Findings

## 2. Prototype `df-harness`

- [ ] 2.1 Create the binary on the spike branch with `agent-client-protocol` as ACP server over stdio and `rpi-agent` as the loop, model configured by base URL and key from the environment (design D3); verify an ACP `initialize` → `session/new` → `session/prompt` smoke test against the spike endpoint streams thought chunks from the reasoning tokens and text chunks and ends with `end_turn`
- [ ] 2.2 Map tool calls, cancellation, usage and the rate-limit stop reason to ACP; verify with the smoke test from 2.1 extended by one tool call, one `session/cancel` and a 429 from a fake OpenAI-compatible endpoint
- [ ] 2.3 Wire the factory MCP server (through `rpi-*` or an `rmcp` bridge) and an `ExecutionEnv` that runs in `/data/repo`; verify `complete_task` ends a run with `COMPLETED` when the runtime starts `df-harness` as its adapter command
- [ ] 2.4 Build `df-harness` as static musl and copy it into a harness image layout `/opt/harness/df-native/bin/df-harness` (`harness-images`); verify it starts in `debian:trixie-slim` and `alpine` and record the binary size in `design.md` Findings

## 3. Measurement and comparison

- [ ] 3.1 Run the fixed task with `df-harness` and with Codex through its ACP adapter, both on the spike endpoint and the same model (D3), 3 runs each, measured as in 1.1; verify both rows in `design.md` Findings are filled in, or Codex's quality columns are marked as not applicable with the reason
- [ ] 3.2 Evaluate all criteria from design D5 and write the decision (build on `rpi-*`, on octolib, or not for now) with the reasons in `design.md` Findings; verify every D5 criterion has a result and the "Decision" line is no longer "pending"

## 4. Follow-up (only if the decision is to build)

- [ ] 4.1 Draft the ADR for full-harness mode as a separate ACP binary (design D1) and update `product/context.md` §4.1 and the backlog entry in `product/iterations/roadmap.md` without assigning an iteration; verify the ADR is listed in `design/adr/README.md`
- [ ] 4.2 Propose the follow-up OpenSpec change for full-harness mode, with an effort estimate per open item from the Findings (compaction, session persistence, custom tools, prompt); verify `openspec validate <follow-up change>` passes
