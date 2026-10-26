# syntax=docker/dockerfile:1
# Claude harness image: the ACP adapter, Node.js, the Claude Code CLI and a musl runtime in a
# `scratch` image. The control plane mounts it read-only at /opt/harness/claude-code in the
# sandbox (openspec change harness-images, design D2). Build the image with this directory as
# context; use test.sh to check it.

FROM node:24-alpine AS build

# The adapter version is also the image tag, the chart default and the df-core default.
ARG CLAUDE_AGENT_ACP_VERSION=0.95.0

# hadolint ignore=DL3018
RUN apk add --no-cache gcc musl-dev
WORKDIR /src
COPY launcher.c .
WORKDIR /out
RUN mkdir -p bin lib \
    && gcc -static -Os -s -o bin/claude-agent-acp /src/launcher.c \
    && ln bin/claude-agent-acp bin/claude \
    && cp -L /lib/ld-musl-*.so.1 /usr/lib/libstdc++.so.6 /usr/lib/libgcc_s.so.1 lib/ \
    && cp /usr/local/bin/node node \
    && npm install --prefix /out --omit=dev --no-audit --no-fund --no-package-lock \
        "@agentclientprotocol/claude-agent-acp@${CLAUDE_AGENT_ACP_VERSION}" \
    && rm -rf /root/.npm

FROM scratch
COPY --from=build /out /
