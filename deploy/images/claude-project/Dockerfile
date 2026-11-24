# syntax=docker/dockerfile:1@sha256:572cebbc2b15e6f84c077742952674889cb63cd5e9b80abdffcbe587b6f19183
# Reference Claude Code project image for Dark Factory sandboxes (openspec change harness-images, design D8).
# Debian Trixie (13) with git, bash, curl, CA certificates and the OpenSSH client, UID 1000 and /workspace.
# It contains no Node.js and no adapter: the control plane mounts the harness image at /opt/harness/claude-code.

FROM debian:trixie-slim@sha256:8aa5a7bf5a1955bddbe7d74fcdd0b91ab30e74362a8b73af672b0481b35b44a0

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
