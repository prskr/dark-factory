# syntax=docker/dockerfile:1
# Reference Claude Code project image for Dark Factory sandboxes (ADR-0024 / design D4).
# Bundles Debian Trixie (13), git, bash, Node.js 22 LTS, and pinned @zed-industries/claude-agent-acp.

FROM node:22-trixie-slim

ARG CLAUDE_AGENT_ACP_VERSION=0.23.1

RUN apt-get update && apt-get install -y --no-install-recommends \
        bash \
        ca-certificates \
        curl \
        git \
        openssh-client \
    && rm -rf /var/lib/apt/lists/* \
    && npm install -g "@zed-industries/claude-agent-acp@${CLAUDE_AGENT_ACP_VERSION}" \
    && npm cache clean --force \
    && mkdir -p /opt/harness/bin /workspace \
    && ln -sf /usr/local/bin/claude-agent-acp /opt/harness/bin/claude-agent-acp \
    && chown -R node:node /workspace

USER 1000:1000
WORKDIR /workspace
