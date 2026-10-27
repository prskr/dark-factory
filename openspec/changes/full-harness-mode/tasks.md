# Tasks

## 1. Harness

- [ ] 1.1 Promote `crates/df-harness` from the spike branch; verify the ACP smoke tests pass in CI
- [ ] 1.2 Usage reporting contract with the runtime; verify per-turn usage appears in the transcript
- [ ] 1.3 Session persistence and `session/load`; verify a resumed run continues the session
- [ ] 1.4 Compaction; verify a long run stays within the context window
- [ ] 1.5 `Retry-After` on rate limits; verify `RATE_LIMITED` carries the retry time
- [ ] 1.6 Prompt and tool tuning (output truncation, custom tools); verify the fixed measurement task and one real repository task pass, the latter with a stronger model than the spike's weak local one (the spike decision requires this quality re-check)
- [ ] 1.7 Harden the `bash` tool through `ExecutionEnv`; verify commands cannot leave the workspace policy
- [ ] 1.8 Track the upstream PR for the `paste` advisory (RUSTSEC-2024-0436); when it lands, drop the `deny.toml` ignore and verify `cargo deny check` passes without it

## 2. Delivery

- [ ] 2.1 Harness image `df-native` and chart wiring; verify it starts in the sandbox image
