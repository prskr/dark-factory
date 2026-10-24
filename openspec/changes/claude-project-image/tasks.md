# Tasks

## 1. Image

- [ ] 1.1 Write `deploy/images/claude-project/test.sh <image>` (design D3); verify it exits non-zero for an image without `/opt/harness/bin/claude-agent-acp` (temporary image that removes the symlink) and for one running as root
- [ ] 1.2 Update the Dockerfile to `node:24-trixie-slim` and `@agentclientprotocol/claude-agent-acp` (`ARG CLAUDE_AGENT_ACP_VERSION`, latest release) (design D2); verify `test.sh` passes for a local `linux/amd64` build

## 2. CI and release

- [ ] 2.1 Add `.github/workflows/claude-project-image.yaml` (design D4: build amd64, run `test.sh`, path filter on `deploy/images/claude-project/**`, actions pinned by commit); verify with `actionlint` and a pull request run
- [ ] 2.2 Add the release job (multi-arch build, `test.sh`, push `:<version>` and `:latest` to `ghcr.io/prskr/dark-factory/claude-project`, version read from the Dockerfile `ARG`); verify with `actionlint` and a `workflow_dispatch` run that pushes the image
- [ ] 2.3 Update `deploy/README.md` §2 (CI-built reference image, tags, how a project points at it, local build with `test.sh`); verify the commands in it by running them

## 3. Dev-cluster verification (carried over from `iter1-egress-gateway` 5.1)

- [ ] 3.1 Run a task with a project that uses the reference image; verify the harness starts (no spawn error) and the run's traffic appears in the egress log
- [ ] 3.2 From a sandbox of that project, request a host that is not allowlisted; verify a 403 and the host in the egress log (`not-allowlisted`) and in the denied-hosts summary
- [ ] 3.3 Add that host to the project's allowlist while the sandbox runs; verify the next request succeeds within about a second
- [ ] 3.4 Send `openssl s_client -proxy <gateway>:3128 -connect <allowlisted host>:443 -servername <other-name>` (CONNECT to an allowed host, ClientHello with another SNI); verify the tunnel is closed before the destination is contacted and logged as `sni-mismatch`
- [ ] 3.5 Request the NativeLink cluster target from the sandbox; verify it is reached directly (no egress log entry)
