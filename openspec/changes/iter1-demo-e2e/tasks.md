# Tasks

## 1. Test setup

- [ ] 1.1 Write `crates/fake-acp-agent/scripts/demo.yaml` and `rate-limit.yaml` (design D3) and add them to the test project image; verify the image contains both under `/opt/harness/scripts/`
- [ ] 1.2 Create `crates/df-e2e` with environment config (database URL, Forgejo URL and bot token, Kubernetes context), per-run isolation (design D2) and cleanup guard; verify a dry run creates and deletes the project and Forgejo repository

## 2. Automated flow

- [ ] 2.1 Implement the "DF-1 with the fake agent" and "tools during the first run" scenarios; verify the test passes against the dev instance
- [ ] 2.2 Implement the "follow-up message" and "complete_task in run 2" scenarios (same PR, session resumed, PR ready); verify the test passes against the dev instance
- [ ] 2.3 Implement the "rate-limited run" scenario with `rate-limit.yaml`; verify the test passes against the dev instance
- [ ] 2.4 Document how to run the suite (`cargo test -p df-e2e -- --ignored`, required env) in `docs/demo.md`; verify the documented command runs as written

## 3. Manual demo with Claude Code

- [ ] 3.1 Run the §2 demo with the Claude project image and a real subscription token, task created and followed up on a phone; use the egress log's denied-hosts view to find the hosts Claude needs and add them as instance allowlist entries; record the host list and any issues in `docs/demo.md`; run the `ANTHROPIC_BASE_URL` check of §7 once (placeholder token, a local pass-through proxy that swaps the bearer) and record whether the adapter works that way
- [ ] 3.2 Confirm every spike in `product/iterations/iteration-1.md` §7 has a recorded result in the owning change's README; list any open follow-ups for iteration 2 in `docs/demo.md`
