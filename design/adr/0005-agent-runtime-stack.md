# ADR-0005: Agent runtime stack

- **Status:** Accepted
- **Date:** 2026-10-16
- **Related:** [ADR-0009](0009-agent-managed-workspaces-git-durable.md), [ADR-0022](0022-project-images-runtime-volume-no-warm-pools.md)

## Context

The runtime inside each sandbox starts and supervises the harness, owns the workspace, proxies tools and streams everything to the control plane. It runs in every sandbox, so its footprint matters. Two modes are planned: ACP client for external harnesses (Claude Code first) and a built-in loop against OpenAI-compatible APIs.

## Decision

| Concern | Choice |
|---|---|
| ACP | `agent-client-protocol` (official Rust SDK) |
| Claude Code | `@zed-industries/claude-agent-acp` (Node package bundling Claude Code); iteration 1: baked into the project image, adapter path configurable |
| Inference (full-harness mode) | Thin reqwest + SSE client (or `async-openai`) speaking the OpenAI-compatible API to the inference endpoint in the gateway Deployment (ADR-0029) |
| MCP | The runtime serves the factory tools to the harness (stdio entry point) and forwards calls to the control plane (ADR-0027). Third-party MCP servers come from the gateway's MCP endpoint, which the runtime registers in the harness configuration as a second server; lazy loading happens there (ADR-0032) |
| Skills | `SKILL.md` format, scanned from `.claude/skills`, `.agents/skills` and a directory the control plane fills |
| Project config | `AGENTS.md` / `CLAUDE.md` accepted as instructions; repo-supplied MCP definitions and hooks not used by the runtime |
| Memory / LSP | Deferred: memory first, then LSP (`async-lsp` + `lsp-types`) |
| Git | `git` CLI with a runtime-owned, allowlisted `.git/config` (ADR-0009) |
| Forge / tracker | Through control-plane tools backed by WASI plugins; the runtime stays forge-agnostic |
| Build / delivery | Static musl binary, mounted via an image volume (ADR-0022) |

## Consequences

- Sandboxes running Claude Code are heavy anyway (Node + bundled CLI); the runtime keeps only its own share small.
- Full-harness mode is easy to start and hard to make good (file editing, output reduction, context management, provider quirks). Before building it, study output reduction as done by tools like `rtk` and existing open-source harnesses (Codex CLI, goose — check licenses).

## Alternatives considered

- **Running Claude Code without ACP** (driving the CLI directly): ACP gives a structured event stream and a path to other harnesses.
