# syntax=docker/dockerfile:1
# Reference Claude Code project image for Dark Factory sandboxes (openspec change harness-images, design D8).
# Debian Trixie (13) with git, bash, curl, CA certificates and the OpenSSH client, UID 1000 and /workspace.
# It contains no Node.js and no adapter: the control plane mounts the harness image at /opt/harness/claude-code.

FROM debian:trixie-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
        bash \
        ca-certificates \
        curl \
        git \
        openssh-client \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 1000 --create-home --shell /bin/bash dev \
    && mkdir -p /workspace \
    && chown 1000:1000 /workspace

USER 1000:1000
WORKDIR /workspace
